'use server'

export async function placeOrder(prev: unknown, formData: FormData) {
  const sku = String(formData.get('sku'))
  return { ok: sku.length > 0 }
}

export async function cancelOrder(id: string) {
  return { cancelled: id }
}
