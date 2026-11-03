export function requireUser(req: any, res: any, next: () => void) {
  if (!req.user) {
    res.status(401).end();
    return;
  }
  next();
}
