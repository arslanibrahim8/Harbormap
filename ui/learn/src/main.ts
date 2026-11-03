// Boots the panel: builds the shell, fetches the first overview and follows the
// live stream. The map itself is drawn into #map by a later module.

import { byId } from "./dom.ts";
import { initCode } from "./code.ts";
import { initDrawer } from "./drawer.ts";
import { loadOverview, startLive } from "./live.ts";
import { renderMap } from "./map.ts";
import { initSidebar } from "./sidebar.ts";
import { initTabs } from "./tabs.ts";
import { initTechStrip } from "./techstrip.ts";
import { initTopbar } from "./topbar.ts";

initTopbar();
initTabs();
initSidebar();
initDrawer();
initCode();
renderMap(byId("map"));
initTechStrip();

loadOverview().catch((e: unknown) => {
  const msg = e instanceof Error ? e.message : "Could not reach the server";
  byId("map").textContent = `⚠ ${msg}`;
});
// The stream reconnects by itself, so it starts even when the first fetch failed.
startLive();
