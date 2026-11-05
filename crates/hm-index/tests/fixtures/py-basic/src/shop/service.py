import logging

from shop.repo import Repo
from . import models
from .mailer import Mailer

log = logging.getLogger(__name__)


class Service:
    repo: Repo

    def __init__(self, mailer: Mailer):
        self.repo = Repo()
        self.mailer = mailer

    def place(self, total):
        order = models.Order(total)
        self.repo.add(order)
        self.mailer.send(order)
        self._audit()
        return order

    def _audit(self):
        log.info("placed")


def place_order(total):
    def check(t):
        return t > 0

    if not check(total):
        raise ValueError("bad total")
    return Service(Mailer()).place(total)
