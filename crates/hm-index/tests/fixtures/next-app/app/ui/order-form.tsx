'use client'

import { useActionState } from 'react'
import { placeOrder } from '../lib/actions'

export function OrderForm() {
  const [state, formAction] = useActionState(placeOrder, null)
  return (
    <form action={formAction}>
      <input name="sku" />
      <button type="submit">Order</button>
      <p>{state ? 'Ordered' : ''}</p>
    </form>
  )
}
