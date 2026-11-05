import { useMutation, useQuery } from '@tanstack/react-query'
import useSWR from 'swr'
import { createOrder, loadOrders } from '../lib/api'

function refresh() {}

function fetcher(url: string) {
  return fetch(url).then((r) => r.json())
}

export function Orders() {
  const add = useMutation({ mutationFn: createOrder, onSuccess: refresh })
  const list = useQuery({ queryKey: ['o'], queryFn: loadOrders })
  const one = useSWR('/api/orders/7', fetcher)
  return <button onClick={() => add.mutate({})}>{list.data?.length}</button>
}
