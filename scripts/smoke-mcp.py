#!/usr/bin/env python3
"""Exercise init, index, and exact MCP searches against a packaged Frigg command."""

import argparse
import contextlib
import json
import os
import selectors
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def run_checked(command: list[str], *args: str, env: dict[str, str]) -> None:
    subprocess.run([*command, *args], check=True, timeout=30, env=env)


def read_response(process: subprocess.Popen[str], request_id: int) -> dict:
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    while selector.select(timeout=30):
        line = process.stdout.readline()
        if not line:
            break
        payload = json.loads(line)
        if payload.get("id") == request_id:
            if "error" in payload:
                raise RuntimeError(f"MCP request {request_id} failed: {payload['error']}")
            return payload["result"]
    raise TimeoutError(f"MCP request {request_id} timed out")


def send(process: subprocess.Popen[str], payload: dict) -> None:
    process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
    process.stdin.flush()


def call_tool(process: subprocess.Popen[str], request_id: int, name: str, arguments: dict) -> dict:
    send(
        process,
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        },
    )
    result = read_response(process, request_id)
    if result.get("isError"):
        raise RuntimeError(f"{name} returned an MCP tool error: {result}")
    structured = result.get("structuredContent")
    if not isinstance(structured, dict):
        raise RuntimeError(f"{name} omitted structuredContent: {result}")
    return structured


class SemanticStubHandler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0"))
        if length <= 0 or length > 1024 * 1024:
            self.send_error(413)
            return
        if self.headers.get("authorization") != "Bearer release-smoke-key":
            self.send_error(401)
            return
        payload = json.loads(self.rfile.read(length))
        inputs = payload.get("input")
        if self.path != "/v1/embeddings" or not isinstance(inputs, list):
            self.send_error(400)
            return
        embedding = [1.0, *([0.0] * 1535)]
        response = json.dumps(
            {
                "data": [
                    {"index": index, "embedding": embedding}
                    for index, _ in enumerate(inputs)
                ],
                "model": payload.get("model", "text-embedding-3-small"),
                "usage": {"prompt_tokens": len(inputs), "total_tokens": len(inputs)},
            },
            separators=(",", ":"),
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)

    def log_message(self, format: str, *args: object) -> None:
        pass


@contextlib.contextmanager
def semantic_stub(enabled: bool):
    if not enabled:
        yield None
        return
    server = ThreadingHTTPServer(("127.0.0.1", 0), SemanticStubHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/v1/embeddings"
    finally:
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--semantic-stub", action="store_true")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a Frigg command is required after --")

    child_env = os.environ.copy()
    child_env.pop("OPENAI_API_KEY", None)
    child_env.pop("GEMINI_API_KEY", None)
    child_env.pop("FRIGG_OPENAI_COMPAT_API_KEY", None)
    with semantic_stub(args.semantic_stub) as semantic_endpoint:
        common = ["--workspace-root", args.workspace, "--watch-mode", "off"]
        if semantic_endpoint is None:
            common.extend(["--semantic-runtime-enabled", "false"])
        else:
            child_env["FRIGG_OPENAI_COMPAT_API_KEY"] = "release-smoke-key"
            common.extend(
                [
                    "--semantic-runtime-enabled",
                    "true",
                    "--semantic-runtime-provider",
                    "openai_compat",
                    "--semantic-runtime-model",
                    "text-embedding-3-small",
                    "--semantic-runtime-openai-compat-endpoint",
                    semantic_endpoint,
                ]
            )
        run_checked(command, *common, "init", env=child_env)
        run_checked(command, *common, "index", env=child_env)

        process = subprocess.Popen(
            [*command, *common],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            env=child_env,
        )
        try:
            send(
                process,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": {"name": "release-smoke", "version": "1"},
                    },
                },
            )
            read_response(process, 1)
            send(process, {"jsonrpc": "2.0", "method": "notifications/initialized"})
            call_tool(process, 2, "workspace", {})
            text = call_tool(process, 3, "search_text", {"query": "ReleaseSmokeSymbol"})
            if not text.get("matches"):
                raise RuntimeError(f"search_text returned no fixture match: {text}")
            symbol = call_tool(process, 4, "search_symbol", {"query": "ReleaseSmokeSymbol"})
            if not symbol.get("matches"):
                raise RuntimeError(f"search_symbol returned no fixture match: {symbol}")
            if not isinstance(symbol.get("metadata"), dict):
                raise RuntimeError(f"search_symbol omitted required metadata: {symbol}")
            if semantic_endpoint is not None:
                hybrid = call_tool(
                    process,
                    5,
                    "search_hybrid",
                    {
                        "query": "semantic release behavior",
                        "semantic": True,
                        "response_mode": "full",
                    },
                )
                metadata = hybrid.get("metadata")
                if not hybrid.get("matches") or not isinstance(metadata, dict):
                    raise RuntimeError(f"search_hybrid omitted semantic results: {hybrid}")
                if metadata.get("semantic_status") != "ok" or not metadata.get(
                    "semantic_enabled"
                ):
                    raise RuntimeError(f"search_hybrid semantic channel was not healthy: {hybrid}")
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


if __name__ == "__main__":
    main()
