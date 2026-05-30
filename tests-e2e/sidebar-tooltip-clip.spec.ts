// Real-browser regression test for the sidebar "(?)" instance-concept
// tooltip clipping bug (queued bug #2, RESOLVED 2026-05-21, squash b9c977b).
//
// Bug: InstanceConceptTooltip.svelte rendered its popover
// `position: absolute` inside the sidebar `<aside>`. The aside is
// `overflow-y: auto`; per the CSS overflow spec, when one axis of
// `overflow` is not `visible` the other axis's `visible` is treated as
// `auto` — so the aside also clips horizontally and grows a horizontal
// scrollbar. A 260px popover wider than the sidebar was clipped at the
// sidebar's right edge and added a horizontal scrollbar to the sidebar.
//
// The fix makes the popover `position: fixed` (anchored to the
// viewport): it escapes the aside's overflow box — not clipped, and
// not part of the aside's scrollable width.
//
// happy-dom (the `pnpm test` environment) has no layout engine, so the
// symptom is invisible there. This test drives real headless Chromium.
//
// The fixture mirrors the structure of InstanceConceptTooltip.svelte
// and its Sidebar host — keep it in sync with those components. The
// popover is `position: fixed`, matching the fixed component; flipping
// it to `position: absolute` reproduces the bug and fails the checks.

import { test, expect } from '@playwright/test';

// Sidebar deliberately narrower than the 260px popover — the bug
// condition. The popover sits inside the sidebar's DOM but is
// `position: fixed`, so it escapes the sidebar's overflow box.
const fixture = `<!DOCTYPE html>
<html>
<body style="margin:0">
  <div style="display:flex">
    <aside id="sidebar"
           style="width:220px;height:300px;overflow-y:auto;background:#fafafa">
      <span>Instance</span>
      <span style="position:relative;display:inline-block">
        <button id="trigger">(?)</button>
        <div id="popover"
             style="position:fixed;top:30px;left:70px;width:260px;
                    background:#fff;border:1px solid #ddd;padding:8px">
          <p>Each instance is a self-contained world.</p>
        </div>
      </span>
      <div style="height:500px">tall filler to make the aside scroll</div>
    </aside>
    <main id="content" style="flex:1;height:300px;background:#fff">content pane</main>
  </div>
</body>
</html>`;

test.describe('Sidebar (?) tooltip clipping regression', () => {
  test('popover escapes the sidebar overflow box', async ({ page }) => {
    await page.setContent(fixture);

    const probe = await page.evaluate(() => {
      const sidebar = document.getElementById('sidebar');
      const popover = document.getElementById('popover');
      const pop = popover!.getBoundingClientRect();
      const aside = sidebar!.getBoundingClientRect();
      // A point near the popover's right edge — past the sidebar's right
      // edge. If the popover is clipped by the sidebar's overflow box,
      // this point shows through to the content pane behind it.
      const probeX = pop.right - 25;
      const probeY = pop.top + pop.height / 2;
      const hit = document.elementFromPoint(probeX, probeY);
      return {
        sidebarScrollWidth: sidebar!.scrollWidth,
        sidebarClientWidth: sidebar!.clientWidth,
        probeBeyondSidebar: probeX > aside.right,
        hitIsPopover: hit ? hit === popover || popover!.contains(hit) : false,
        hitDesc: hit ? (hit as HTMLElement).id || hit.tagName : 'none',
      };
    });

    expect(probe.sidebarScrollWidth, 'sidebar has no horizontal scrollbar').toBeLessThanOrEqual(
      probe.sidebarClientWidth,
    );
    expect(probe.probeBeyondSidebar, 'fixture geometry is meaningful').toBe(true);
    expect(probe.hitIsPopover, 'popover is not clipped by sidebar').toBe(true);
  });
});
