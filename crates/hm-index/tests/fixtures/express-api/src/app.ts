import express from 'express';
import orders from './routes/orders';
import { requireUser } from './auth';

const app = express();

app.use(requireUser);
app.use('/api', orders);
app.get('/health', (req, res) => res.send('ok'));

export default app;
