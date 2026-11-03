export function runPlugin(code: string, name: string) {
  eval(code);
  const key = process.env.SHOP_KEY;
  return import(name).then((m) => m.default(key));
}
