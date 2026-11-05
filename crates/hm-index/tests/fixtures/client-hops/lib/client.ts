export const client = {
  get(options: { url: string }) {
    return fetch(options.url)
  },
}
