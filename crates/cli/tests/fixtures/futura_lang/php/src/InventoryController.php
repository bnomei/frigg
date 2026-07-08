<?php

declare(strict_types=1);

namespace Futura\Lang\Php;

/** HTTP-shaped controller for text/symbol probes on the PHP lang board. */
final class InventoryController
{
    public function __construct(
        private readonly OrderService $orders,
    ) {
    }

    public function restock(string $sku, int $qty): array
    {
        $order = $this->orders->placeOrder($sku, $qty);
        return [
            'status' => 'restocked',
            'sku' => $order->sku,
            'qty' => $order->qty,
            'marker' => 'FUTURA_LANG_PHP_RESTOCK_MARKER',
        ];
    }
}
