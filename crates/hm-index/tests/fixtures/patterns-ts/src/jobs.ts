import { Queue, Worker } from "bullmq";
import { revalidatePath } from "next/cache";
import { cookies } from "next/headers";

const mail = new Queue("mail");
export const worker = new Worker("mail", async () => {});

export async function refresh() {
  revalidatePath("/orders");
  const jar = await cookies();
  return fetch("https://example.test/ping", { signal: AbortSignal.timeout(5000) });
}
