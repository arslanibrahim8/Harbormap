import { sql } from "drizzle-orm";
import { sign } from "node:crypto";

export function total() {
  sign(null, Buffer.from("x"), "key");
  return sql`sum(price)`;
}
