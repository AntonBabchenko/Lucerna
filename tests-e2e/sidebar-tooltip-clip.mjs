// Real-browser regression test for the sidebar "(?)" instance-concept
// tooltip clipping bug (queued bug #2).
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
//
// Run: pnpm test:e2e   (exit code 0 = pass, 1 = fail)

import { chromium } from 'playwright';

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

const checks = [];
function check(name, ok, detail) {
  checks.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : `  -> ${detail}`}`);
}

const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  await page.setContent(fixture);

  const probe = await page.evaluate(() => {
    const sidebar = document.getElementById('sidebar');
    const popover = document.getElementById('popover');
    const pop = popover.getBoundingClientRect();
    const aside = sidebar.getBoundingClientRect();
    // A point near the popover's right edge — past the sidebar's right
    // edge. If the popover is clipped by the sidebar's overflow box,
    // this point shows through to the content pane behind it.
    const probeX = pop.right - 25;
    const probeY = pop.top + pop.height / 2;
    const hit = document.elementFromPoint(probeX, probeY);
    return {
      sidebarScrollWidth: sidebar.scrollWidth,
      sidebarClientWidth: sidebar.clientWidth,
      probeBeyondSidebar: probeX > aside.right,
      hitIsPopover: hit ? hit === popover || popover.contains(hit) : false,
      hitDesc: hit ? hit.id || hit.tagName : 'none',
    };
  });

  check(
    'sidebar has no horizontal scrollbar',
    probe.sidebarScrollWidth <= probe.sidebarClientWidth,
    `scrollWidth ${probe.sidebarScrollWidth} > clientWidth ${probe.sidebarClientWidth}`,
  );
  check(
    'probe point lies beyond the sidebar (fixture geometry is meaningful)',
    probe.probeBeyondSidebar === true,
    'probe point fell inside the sidebar — fixture geometry is wrong',
  );
  check(
    'popover is not clipped by the sidebar',
    probe.hitIsPopover === true,
    `a point past the sidebar edge hit "${probe.hitDesc}", not the popover`,
  );
} finally {
  await browser.close();
}

const failed = checks.filter((c) => !c.ok).length;
console.log('');
console.log(`${checks.length - failed}/${checks.length} checks passed`);
if (failed > 0) {
  console.error('REGRESSION: sidebar (?) tooltip is clipped / adds a scrollbar.');
  process.exit(1);
}
console.log('OK — sidebar (?) tooltip escapes the sidebar overflow box.');
