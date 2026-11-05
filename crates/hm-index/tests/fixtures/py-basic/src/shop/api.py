import shop.models
from shop import Order, place_order
from shop.fast import speedup


def route(path):
    def wrap(fn):
        return fn

    return wrap


@route("/orders")
def create(total):
    order = place_order(speedup(total))
    shop.models.Order(total)
    return Order
