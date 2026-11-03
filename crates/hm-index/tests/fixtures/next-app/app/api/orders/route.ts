export async function GET() {
  return Response.json([])
}

export const POST = async (request: Request) => {
  const order = await request.json()
  return Response.json(order, { status: 201 })
}
