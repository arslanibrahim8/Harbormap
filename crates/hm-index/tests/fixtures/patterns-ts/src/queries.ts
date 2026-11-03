import { eq } from "drizzle-orm";

function select(rows: unknown[]) {
  return rows;
}

export function lookups(searchParams: URLSearchParams, db: any, users: unknown) {
  select([]);
  searchParams.delete();
  db.select().from(users).where(eq(1, 1));
  db.delete(users);
}
