export function Inline({ id }: { id: string }) {
  async function save() {
    'use server'
    await Promise.resolve(id)
  }
  return (
    <form action={save}>
      <button type="submit">Save</button>
    </form>
  )
}
