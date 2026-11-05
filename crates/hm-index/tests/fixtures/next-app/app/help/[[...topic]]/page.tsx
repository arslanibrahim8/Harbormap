export default async function HelpPage({ params }: { params: Promise<{ topic?: string[] }> }) {
  const { topic } = await params
  return <h1>{topic ? topic.join('/') : 'Help'}</h1>
}
