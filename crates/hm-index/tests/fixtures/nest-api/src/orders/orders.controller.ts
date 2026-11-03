import { Controller, Get, Post, UseGuards } from '@nestjs/common';
import { AuthGuard } from '../auth.guard';
import { OrdersService } from './orders.service';

@Controller('orders')
export class OrdersController {
  constructor(private readonly orders: OrdersService) {}

  @Get()
  list() {
    return this.orders.findAll();
  }

  @Get(':id')
  one() {
    return null;
  }

  @Post()
  @UseGuards(AuthGuard)
  place() {
    return {};
  }
}
