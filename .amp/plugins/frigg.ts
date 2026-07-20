import type {
  PluginAPI,
  PluginCommandContext,
  PluginUI,
} from "@ampcode/plugin";

const DEFAULT_ENDPOINT = "http://127.0.0.1:37444";
const DOCS_URL = "https://github.com/bnomei/frigg#readme";
const CHECK_TTL_MS = 15_000;
const REQUEST_TIMEOUT_MS = 1_500;

interface FriggPluginConfig {
  frigg?: {
    binary?: string;
    endpoint?: string;
    softNudge?: boolean;
    notifyOnFailure?: boolean;
  };
}

interface ResolvedConfig {
  binary: string;
  endpoint: string;
  softNudge: boolean;
  notifyOnFailure: boolean;
}

interface HealthResponse {
  schema_version: number;
  status: string;
  frigg_version: string;
}

interface RepositoryStatus {
  display_name: string;
  root_path: string;
  storage?: {
    index_state: string;
    error?: string;
  };
}

interface ServiceStatus {
  schema_version: number;
  frigg_version: string;
  repositories: RepositoryStatus[];
  runtime: {
    profile: string;
    watch_active: boolean;
    watch_status?: {
      reason: string;
      lease_count: number;
    };
    tool_surface_profile: string;
    tools_exposed: string[];
    active_tasks?: unknown[];
  };
}

interface LocalStatus {
  schema_version: number;
  frigg_version: string;
  watch: {
    configured_mode: string;
  };
  repositories: Array<{
    display_name: string;
    storage: {
      index_state: string;
      error?: string;
    };
  }>;
}

type ServiceCheck =
  | {
      ok: true;
      endpoint: string;
      health: HealthResponse;
      status: ServiceStatus;
    }
  | {
      ok: false;
      endpoint: string;
      reason: string;
    };

interface CachedCheck {
  checkedAt: number;
  result: ServiceCheck;
}

export default function friggPlugin(amp: PluginAPI) {
  const checks = new Map<string, CachedCheck>();
  const serviceStates = new Map<string, boolean>();
  const nudgedTurns = new Set<string>();

  const readConfig = async (): Promise<ResolvedConfig> => {
    const root = (await amp.configuration.get()) as FriggPluginConfig;
    const configured = root.frigg;
    return {
      binary: nonEmptyString(configured?.binary) ?? "frigg",
      endpoint: normalizeEndpoint(configured?.endpoint),
      softNudge: configured?.softNudge !== false,
      notifyOnFailure: configured?.notifyOnFailure !== false,
    };
  };

  const checkService = async (
    config: ResolvedConfig,
    force = false,
  ): Promise<ServiceCheck> => {
    const workspace = amp.system.workspaceRoot?.toString() ?? "(no-workspace)";
    const cacheKey = `${workspace}\n${config.endpoint}`;
    const cached = checks.get(cacheKey);
    if (!force && cached && Date.now() - cached.checkedAt < CHECK_TTL_MS) {
      return cached.result;
    }

    const result = await fetchServiceStatus(config.endpoint);
    checks.set(cacheKey, { checkedAt: Date.now(), result });
    return result;
  };

  amp.registerCommand(
    "frigg-show-status",
    {
      title: "Show status",
      category: "Frigg",
      description:
        "Show Frigg service, repository, watch, and tool-surface status.",
    },
    async (ctx) => {
      const config = await readConfig();
      const unsupported = loopbackExecutorWarning(amp, config.endpoint);
      if (unsupported) {
        await notify(amp, ctx.ui, unsupported);
        return;
      }

      const result = await checkService(config, true);
      const localStatus =
        amp.system.executor.kind === "local"
          ? await inspectLocalStatus(amp, ctx, config)
          : undefined;
      await notify(amp, ctx.ui, formatStatus(result, localStatus));
    },
  );

  amp.registerCommand(
    "frigg-check-setup",
    {
      title: "Check setup",
      category: "Frigg",
      description:
        "Check the Frigg binary, service, workspace index, and Amp skill.",
    },
    async (ctx) => {
      const config = await readConfig();
      await notify(
        amp,
        ctx.ui,
        await formatSetupCheck(amp, ctx, config, checkService),
      );
    },
  );

  amp.registerCommand(
    "frigg-open-documentation",
    {
      title: "Open documentation",
      category: "Frigg",
      description: "Open the Frigg documentation.",
    },
    async (ctx) => {
      await ctx.system.open(DOCS_URL);
    },
  );

  amp.on("agent.start", async (event) => {
    const config = await readConfig();
    if (
      !config.softNudge ||
      loopbackExecutorWarning(amp, config.endpoint) ||
      !shouldNudge(event.message)
    )
      return {};
    if (/frigg-first-code-search/i.test(event.message)) return {};

    const turnKey = `${event.thread.id}:${event.id}`;
    if (nudgedTurns.has(turnKey)) return {};
    nudgedTurns.add(turnKey);
    if (nudgedTurns.size > 256)
      nudgedTurns.delete(nudgedTurns.values().next().value!);

    return {
      message: {
        content:
          "For source-code discovery in this workspace, load the frigg-first-code-search skill before broad shell search or file reads.",
        display: false,
      },
    };
  });

  amp.on("session.start", async (event, ctx) => {
    const config = await readConfig();
    if (
      !config.notifyOnFailure ||
      loopbackExecutorWarning(amp, config.endpoint)
    )
      return;

    const result = await checkService(config);
    const stateKey = `${amp.system.workspaceRoot?.toString() ?? "(no-workspace)"}\n${config.endpoint}`;
    if (amp.activeThread.current?.id !== event.thread.id) return;
    const previous = serviceStates.get(stateKey);
    serviceStates.set(stateKey, result.ok);
    if (!result.ok && previous !== false) {
      await notify(
        amp,
        ctx.ui,
        `Frigg is unavailable at ${config.endpoint}: ${result.reason}\nRun \`frigg serve\` in this workspace, then run “Frigg: Check setup”.`,
      );
    } else if (result.ok && previous === false) {
      await notify(
        amp,
        ctx.ui,
        `Frigg is available again at ${config.endpoint}.`,
      );
    }
  });
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function normalizeEndpoint(value: unknown): string {
  const endpoint = nonEmptyString(value) ?? DEFAULT_ENDPOINT;
  return endpoint.replace(/\/+$/, "");
}

function isLoopbackEndpoint(endpoint: string): boolean {
  try {
    const hostname = new URL(endpoint).hostname.toLowerCase();
    return (
      hostname === "127.0.0.1" ||
      hostname === "localhost" ||
      hostname === "::1" ||
      hostname === "[::1]"
    );
  } catch {
    return false;
  }
}

function loopbackExecutorWarning(
  amp: PluginAPI,
  endpoint: string,
): string | undefined {
  if (amp.system.executor.kind !== "remote" || !isLoopbackEndpoint(endpoint))
    return undefined;
  return [
    `Frigg endpoint ${endpoint} is loopback, but this plugin is running on a remote executor.`,
    "Configure frigg.endpoint for a service reachable from that executor; the plugin will not assume access to your local machine.",
  ].join("\n");
}

async function fetchServiceStatus(endpoint: string): Promise<ServiceCheck> {
  let baseURL: URL;
  try {
    baseURL = new URL(endpoint);
    if (
      (baseURL.protocol !== "http:" && baseURL.protocol !== "https:") ||
      baseURL.username ||
      baseURL.password ||
      baseURL.search ||
      baseURL.hash
    ) {
      throw new Error("unsupported endpoint URL");
    }
  } catch {
    return { ok: false, endpoint, reason: "invalid frigg.endpoint URL" };
  }

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const authToken = nonEmptyString(process.env.FRIGG_MCP_HTTP_AUTH_TOKEN);
    const headers = authToken
      ? { Authorization: `Bearer ${authToken}` }
      : undefined;
    const [healthResponse, statusResponse] = await Promise.all([
      fetch(new URL("healthz", `${baseURL.toString()}/`), {
        signal: controller.signal,
        headers,
      }),
      fetch(new URL("status", `${baseURL.toString()}/`), {
        signal: controller.signal,
        headers,
      }),
    ]);
    if (!healthResponse.ok) {
      return {
        ok: false,
        endpoint,
        reason: `/healthz returned HTTP ${healthResponse.status}`,
      };
    }
    if (!statusResponse.ok) {
      return {
        ok: false,
        endpoint,
        reason: `/status returned HTTP ${statusResponse.status}`,
      };
    }

    const health = (await healthResponse.json()) as HealthResponse;
    const status = (await statusResponse.json()) as ServiceStatus;
    if (!isHealthResponse(health) || !isServiceStatus(status)) {
      return {
        ok: false,
        endpoint,
        reason: "service returned an unsupported or invalid status schema",
      };
    }
    return { ok: true, endpoint, health, status };
  } catch (error) {
    const reason =
      error instanceof Error && error.name === "AbortError"
        ? "request timed out"
        : errorMessage(error);
    return { ok: false, endpoint, reason };
  } finally {
    clearTimeout(timeout);
  }
}

function formatStatus(
  result: ServiceCheck,
  localStatus?: LocalStatus | string,
): string {
  if (!result.ok) {
    return [
      `✗ Frigg unavailable · ${shortText(result.reason, 80)}`,
      typeof localStatus === "object"
        ? formatLocalStatusLine(localStatus)
        : (localStatus ?? `Endpoint: ${result.endpoint}`),
      "Run `frigg serve` and retry.",
    ].join("\n");
  }

  const { status } = result;
  const watch =
    status.runtime.watch_status?.reason ??
    (status.runtime.watch_active ? "active" : "off");
  const activeTasks = status.runtime.active_tasks?.length ?? 0;

  return [
    `✓ Frigg ${status.frigg_version} · ${status.runtime.profile.replaceAll("_", " ")}`,
    typeof localStatus === "object"
      ? formatLocalStatusLine(localStatus)
      : (localStatus ?? `Endpoint: ${result.endpoint}`),
    `Watch: ${watch.replaceAll("_", " ")} · ${status.runtime.tools_exposed.length} tools${activeTasks > 0 ? ` · ${activeTasks} active tasks` : ""}`,
  ].join("\n");
}

async function formatSetupCheck(
  amp: PluginAPI,
  ctx: PluginCommandContext,
  config: ResolvedConfig,
  checkService: (
    config: ResolvedConfig,
    force?: boolean,
  ) => Promise<ServiceCheck>,
): Promise<string> {
  const lines = [`Frigg setup (${amp.system.executor.kind} executor)`];
  const executorWarning = loopbackExecutorWarning(amp, config.endpoint);

  if (amp.system.executor.kind === "local") {
    try {
      const binary = await ctx.$`${config.binary} --version`;
      if (binary.exitCode === 0) {
        lines.push(`✓ Binary: ${binary.stdout.trim() || config.binary}`);
      } else {
        lines.push(
          `✗ Binary: ${config.binary} is unavailable; install Frigg and ensure it is on PATH.`,
        );
      }
    } catch {
      lines.push(
        `✗ Binary: ${config.binary} is unavailable; install Frigg and ensure it is on PATH.`,
      );
    }

    const localStatus = await inspectLocalStatus(amp, ctx, config);
    lines.push(
      typeof localStatus === "object"
        ? formatLocalSetupStatus(localStatus)
        : localStatus,
    );
  } else {
    lines.push("– Binary: skipped outside the local executor");
  }

  if (executorWarning) {
    lines.push(`✗ Service: ${executorWarning}`);
  } else {
    const result = await checkService(config, true);
    if (!result.ok) {
      lines.push(
        `✗ Service: ${result.reason}; run \`frigg serve\` in the indexed workspace.`,
      );
    } else {
      lines.push(
        `✓ Service: Frigg ${result.status.frigg_version} at ${result.endpoint}`,
      );
    }
  }

  if (amp.system.executor.kind === "local") {
    const skill = await inspectSkillInstallation(amp, config.endpoint);
    lines.push(skill);
  } else {
    lines.push("– Amp skill: skipped outside the local executor");
  }

  return lines.join("\n");
}

async function inspectLocalStatus(
  amp: PluginAPI,
  ctx: PluginCommandContext,
  config: ResolvedConfig,
): Promise<LocalStatus | string> {
  const workspace = localWorkspacePath(amp);
  if (!workspace) return "– Workspace: no local Amp workspace is open.";

  try {
    const result =
      await ctx.$`${config.binary} --workspace-root ${workspace} status --json`;
    if (result.exitCode !== 0) {
      return formatLocalStatusFailure(result.stderr);
    }
    const status = JSON.parse(result.stdout) as LocalStatus;
    if (!isLocalStatus(status)) {
      return "✗ Workspace: `frigg status --json` returned an unsupported schema.";
    }
    return status;
  } catch (error) {
    return formatLocalStatusFailure(errorMessage(error));
  }
}

function formatLocalStatusFailure(detail: string): string {
  if (/unrecognized subcommand\s+['"]status['"]/i.test(detail)) {
    return "✗ Local index: update Frigg CLI (`status` missing)";
  }

  const summary = detail
    .split("\n")
    .map((line) => line.trim())
    .find(Boolean);
  return `✗ Local index: status failed${summary ? ` · ${shortText(summary, 80)}` : ""}`;
}

function formatLocalStatus(status: LocalStatus): string {
  const ready = status.repositories.filter(
    (repository) => repository.storage.index_state === "ready",
  ).length;
  const readiness =
    status.repositories.length === 1
      ? status.repositories[0]!.storage.index_state
      : `${ready}/${status.repositories.length} ready`;
  return `Local index: ${readiness} · watch ${status.watch.configured_mode}`;
}

function formatLocalStatusLine(status: LocalStatus): string {
  return `${localStatusIsReady(status) ? "✓" : "✗"} ${formatLocalStatus(status)}`;
}

function formatLocalSetupStatus(status: LocalStatus): string {
  const ready = localStatusIsReady(status);
  return `${ready ? "✓" : "✗"} ${formatLocalStatus(status)}${ready ? "" : "; run `frigg index` before searching."}`;
}

function localStatusIsReady(status: LocalStatus): boolean {
  return (
    status.repositories.length > 0 &&
    status.repositories.every(
      (repository) => repository.storage.index_state === "ready",
    )
  );
}

function shortText(value: string, maxLength: number): string {
  const firstLine = value.split("\n", 1)[0]!.trim();
  return firstLine.length <= maxLength
    ? firstLine
    : `${firstLine.slice(0, maxLength - 1)}…`;
}

async function inspectSkillInstallation(
  amp: PluginAPI,
  endpoint: string,
): Promise<string> {
  const workspace = localWorkspacePath(amp);
  const home = process.env.HOME;
  const roots = [
    home ? `${home}/.config/agents/skills/frigg-first-code-search` : undefined,
    home ? `${home}/.config/amp/skills/frigg-first-code-search` : undefined,
    workspace
      ? `${workspace}/.agents/skills/frigg-first-code-search`
      : undefined,
  ].filter((path): path is string => Boolean(path));

  for (const root of roots) {
    if (!(await Bun.file(`${root}/SKILL.md`).exists())) continue;
    const mcpFile = Bun.file(`${root}/mcp.json`);
    if (!(await mcpFile.exists())) {
      return `✗ Amp skill: installed at ${root}, but mcp.json is missing; run \`frigg adopt --skill-provider amp\`.`;
    }
    try {
      const mcp = (await mcpFile.json()) as { frigg?: { url?: string } };
      const mcpURL = nonEmptyString(mcp.frigg?.url);
      if (
        !mcpURL ||
        normalizeEndpoint(mcpURL.replace(/\/mcp$/, "")) !== endpoint
      ) {
        return `✗ Amp skill: mcp.json uses ${mcp.frigg?.url ?? "no Frigg URL"}, not ${endpoint}/mcp.`;
      }
      return `✓ Amp skill: ${root}`;
    } catch {
      return `✗ Amp skill: ${root}/mcp.json is invalid JSON.`;
    }
  }

  return "✗ Amp skill: not installed; run `frigg adopt --skill-provider amp`.";
}

function localWorkspacePath(amp: PluginAPI): string | undefined {
  if (amp.system.executor.kind !== "local" || !amp.system.workspaceRoot)
    return undefined;
  return amp.helpers.filePathFromURI(amp.system.workspaceRoot);
}

function isHealthResponse(value: unknown): value is HealthResponse {
  return (
    isRecord(value) &&
    value.schema_version === 1 &&
    value.status === "ok" &&
    typeof value.frigg_version === "string"
  );
}

function isServiceStatus(value: unknown): value is ServiceStatus {
  if (
    !isRecord(value) ||
    value.schema_version !== 1 ||
    typeof value.frigg_version !== "string" ||
    !Array.isArray(value.repositories) ||
    !isRecord(value.runtime)
  )
    return false;

  const runtime = value.runtime;
  const repositoriesValid = value.repositories.every(
    (repository) =>
      isRecord(repository) &&
      typeof repository.display_name === "string" &&
      typeof repository.root_path === "string" &&
      (repository.storage === undefined ||
        (isRecord(repository.storage) &&
          typeof repository.storage.index_state === "string")),
  );
  const watchStatusValid =
    runtime.watch_status === undefined ||
    (isRecord(runtime.watch_status) &&
      typeof runtime.watch_status.reason === "string" &&
      typeof runtime.watch_status.lease_count === "number");
  return (
    repositoriesValid &&
    typeof runtime.profile === "string" &&
    typeof runtime.watch_active === "boolean" &&
    watchStatusValid &&
    typeof runtime.tool_surface_profile === "string" &&
    Array.isArray(runtime.tools_exposed) &&
    runtime.tools_exposed.every((tool) => typeof tool === "string") &&
    (runtime.active_tasks === undefined || Array.isArray(runtime.active_tasks))
  );
}

function isLocalStatus(value: unknown): value is LocalStatus {
  return (
    isRecord(value) &&
    value.schema_version === 1 &&
    typeof value.frigg_version === "string" &&
    isRecord(value.watch) &&
    typeof value.watch.configured_mode === "string" &&
    Array.isArray(value.repositories) &&
    value.repositories.every(
      (repository) =>
        isRecord(repository) &&
        typeof repository.display_name === "string" &&
        isRecord(repository.storage) &&
        typeof repository.storage.index_state === "string",
    )
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function shouldNudge(message: string): boolean {
  return /\b(?:code review|review (?:this |the )?(?:code|diff|changes)|debug(?:ging)? (?:this |the )?(?:code|bug|error|failure)|trace (?:a |the )?(?:bug|error|failure)|where (?:is|are) .+ (?:defined|implemented|used|in (?:this |the )?(?:repo|repository|codebase))|find (?:the )?(?:implementation|definition|references?|callers?|symbol)|search (?:the )?(?:code|source|repo|repository|codebase)|locate (?:the )?(?:code|implementation|definition|symbol)|how does .+ work (?:in|inside) (?:this |the )?(?:repo|repository|codebase))\b/i.test(
    message,
  );
}

async function notify(
  amp: PluginAPI,
  ui: PluginUI,
  message: string,
): Promise<void> {
  try {
    await ui.notify(message);
  } catch (error) {
    if (
      error instanceof Error &&
      amp.helpers.isPluginUINotAvailableError(error)
    ) {
      amp.logger.log(message);
      return;
    }
    amp.logger.log("Frigg notification failed:", errorMessage(error));
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
