import { sql } from "@vercel/postgres";

export async function countOrders() {
  return sql`select count(*) from orders`;
}
