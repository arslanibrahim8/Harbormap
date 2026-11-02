import { z } from "zod";
import { db } from "../db";
import * as mail from "./mail";

export function placeOrder(id: number) {
  const order = db.insert(id);
  mail.sendReceipt(order);
  return z.object({}).parse(order);
}

export class OrderService {
  static find(id: number) {
    return db.get(id);
  }
  total() {
    return this.sum();
  }
  sum() {
    return 0;
  }
}
