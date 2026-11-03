package store

import "example.com/shop"

type Memory struct {
	orders map[int]*shop.Order
}

func NewMemory() *Memory {
	return &Memory{orders: make(map[int]*shop.Order)}
}

func (m *Memory) Save(o *shop.Order) error {
	m.orders[o.ID] = o
	return nil
}

func (m *Memory) Find(id int) (*shop.Order, error) {
	return m.lookup(id), nil
}

func (m *Memory) lookup(id int) *shop.Order {
	return m.orders[id]
}
