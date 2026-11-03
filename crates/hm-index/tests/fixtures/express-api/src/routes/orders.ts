import { Router } from 'express';
import { listOrders, placeOrder } from '../orders';

const router = Router();

function validate(req: any, res: any, next: () => void) {
  if (!req.body) {
    res.status(400).end();
    return;
  }
  next();
}

router.get('/orders', listOrders);
router.post('/orders', validate, placeOrder);
router.delete('/orders/:id', async (req, res) => {
  await removeOrder(req.params.id);
  res.status(204).end();
});

async function removeOrder(id: string) {
  return id;
}

export default router;
