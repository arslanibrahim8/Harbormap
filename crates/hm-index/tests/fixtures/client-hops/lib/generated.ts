import { client } from './client'

export class OrdersService {
  static list() {
    return client.get({ url: '/api/orders' })
  }
}
