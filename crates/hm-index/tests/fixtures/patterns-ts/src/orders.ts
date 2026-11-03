import { PrismaClient } from "@prisma/client";
import { z } from "zod";
import bcrypt from "bcryptjs";
import jwt from "jsonwebtoken";

const prisma = new PrismaClient();
const OrderInput = z.object({ sku: z.string() });

export async function listOrders() {
  return prisma.order.findMany();
}

export async function placeOrder(body: unknown, raw: string) {
  const input = OrderInput.parse(body);
  const sku = z.string().parse(raw);
  const parsed = JSON.parse(raw);
  await prisma.$transaction([prisma.order.create({ data: input })]);
  return { sku, parsed };
}

export async function signUp(password: string) {
  const hash = await bcrypt.hash(password, 10);
  return jwt.sign({ hash }, process.env.SHOP_SECRET as string);
}

export function evaluate(code: string) {
  return eval(code);
}

export function forget(cache: Map<string, string>, params: URLSearchParams) {
  cache.delete("orders");
  params.delete("page");
}
