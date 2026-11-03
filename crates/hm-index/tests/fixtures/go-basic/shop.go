package shop

import "errors"

// ErrBad is returned for an order with a negative total.
var ErrBad = errors.New("bad order")

type Order struct {
	ID    int
	Total int
}

type OrderStore interface {
	Save(o *Order) error
	Find(id int) (*Order, error)
}

type Notifier interface {
	Notify(msg string)
}

func Validate(o *Order) error {
	if o.Total < 0 {
		return ErrBad
	}
	return nil
}
