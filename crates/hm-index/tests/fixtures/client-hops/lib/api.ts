import axios from 'axios'
import { BASE } from './config'

export async function loadOrders() {
  return fetch('/api/orders')
}

export function removeOrder(id: string) {
  return fetch(`/api/orders/${id}`, { method: 'DELETE' })
}

export const createOrder = (o) => axios.post('/api/orders', o)

export function fromVar(u: string) {
  return fetch(u)
}

export function elsewhere() {
  return fetch(`${BASE}/api/orders`)
}
