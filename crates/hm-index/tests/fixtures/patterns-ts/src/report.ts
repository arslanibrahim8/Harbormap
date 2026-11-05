import { sql } from "drizzle-orm";
import { sign } from "node:crypto";
import jwt from "jsonwebtoken";

export function total() {
  sign(null, Buffer.from("x"), "key");
  jwt.sign({ total: 1 }, "key");
  return sql`sum(price)`;
}
