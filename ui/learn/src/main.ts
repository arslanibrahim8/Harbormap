// Boot stub: proves the bundle loads and the server answers. The real shell
// (map, drawer, search) replaces this body in the next task.

import { overview } from "./api.ts";

async function boot(app: HTMLElement): Promise<void> {
  try {
    const o = await overview();
    app.textContent = o.repo.name;
  } catch (e) {
    app.textContent = e instanceof Error ? e.message : "Could not reach the server";
  }
}

const app = document.getElementById("app");
if (app !== null) void boot(app);
