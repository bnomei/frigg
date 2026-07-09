"""Multi-language gate fixture: Python inventory module."""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class StockItem:
    sku: str
    qty: int


class InventoryService:
    """Tracks SKU quantities for multi-language bench probes."""

    def __init__(self) -> None:
        self._items: dict[str, StockItem] = {}

    def restock(self, sku: str, qty: int) -> StockItem:
        existing = self._items.get(sku)
        if existing is None:
            item = StockItem(sku=sku, qty=qty)
        else:
            item = StockItem(sku=sku, qty=existing.qty + qty)
        self._items[sku] = item
        return item

    def get(self, sku: str) -> StockItem | None:
        return self._items.get(sku)


FUTURA_LANG_PY_MARKER = "FUTURA_LANG_PY_INVENTORY_MARKER"
