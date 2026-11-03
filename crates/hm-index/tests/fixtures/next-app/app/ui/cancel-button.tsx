import { cancelOrder } from '../lib/actions'

export function CancelButton({ id }: { id: string }) {
  const cancelWithId = cancelOrder.bind(null, id)
  return (
    <form action={cancelWithId}>
      <button type="submit">Cancel</button>
    </form>
  )
}
