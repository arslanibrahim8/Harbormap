import { useRouter } from 'next/router'

export default function OrderPage() {
  const router = useRouter()
  return <h1>Order {router.query.id}</h1>
}
