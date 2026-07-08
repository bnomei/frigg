"""Secondary Python module for multi-file symbol/text probes."""

from inventory import InventoryService, StockItem

FUTURA_LANG_PY_WAREHOUSE_MARKER = "FUTURA_LANG_PY_WAREHOUSE_MARKER"


class Warehouse:
    def __init__(self, inventory: InventoryService) -> None:
        self.inventory = inventory

    def receive(self, sku: str, qty: int) -> StockItem:
        return self.inventory.restock(sku, qty)
