from .models import Order


class Repo:
    def add(self, order: Order) -> None:
        order.save()

    def all(self):
        return []
