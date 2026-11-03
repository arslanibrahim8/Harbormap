export function Search() {
  return (
    <div>
      <form action="/search" method="get">
        <input name="q" />
      </form>
      <form onSubmit={(event) => event.preventDefault()}>
        <input name="note" />
      </form>
    </div>
  )
}
