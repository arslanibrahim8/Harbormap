export default async function OrderPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params
  return <h1>Order {id}</h1>
}
