class Base:
    def save(self):
        return self.validate()

    def validate(self):
        return True


class Order(Base):
    def __init__(self, total: int):
        self.total = total

    def validate(self):
        return self.total >= 0

    def persist(self):
        return self.save()
