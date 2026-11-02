const { placeOrder } = require("./orders/service");
module.exports = function run() {
  placeOrder(3);
};
