<?php

declare(strict_types=1);

namespace Futura\Lang\Php;

/**
 * Multi-language gate fixture: PHP order service (not a product tree).
 */
final class OrderService
{
    public function __construct(
        private readonly OrderRepository $repository,
    ) {
    }

    public function placeOrder(string $sku, int $qty): Order
    {
        $order = new Order($sku, $qty);
        $this->repository->save($order);
        return $order;
    }

    public function findBySku(string $sku): ?Order
    {
        return $this->repository->findBySku($sku);
    }
}

final class Order
{
    public function __construct(
        public readonly string $sku,
        public readonly int $qty,
    ) {
    }
}

interface OrderRepository
{
    public function save(Order $order): void;

    public function findBySku(string $sku): ?Order;
}
