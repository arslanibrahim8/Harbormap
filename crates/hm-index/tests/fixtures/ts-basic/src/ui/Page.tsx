import Button from "./Button";
import { OrderService, receipt } from "@/orders";
export function Page() {
  OrderService.find(2);
  receipt(null);
  return <Button />;
}
