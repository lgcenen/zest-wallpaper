import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const styles = readFileSync(join(process.cwd(), "src/styles.css"), "utf8");

describe("phase-06 style contract", () => {
  it("keeps the library thumbnails square and the workbench as one continuous shell", () => {
    expect(styles).toMatch(/\.workbench-shell \.library-card-thumb\s*{[\s\S]*padding-bottom:\s*100%;/);
    expect(styles).toContain(".workbench-shell .settings-popover");
    expect(styles).toContain(".workbench-shell .library-card-overlay");
    expect(styles).toMatch(/\.workbench-shell\s*{[\s\S]*gap:\s*0;/);
    expect(styles).toMatch(/\.workbench-shell\s*{[\s\S]*padding:\s*0;/);
    expect(styles).toMatch(/:root\s*{[\s\S]*--wb-gui-opacity:\s*1;/);
    expect(styles).toMatch(/html\[data-workbench-theme\] body\s*{[\s\S]*background:\s*transparent;/);
    expect(styles).toMatch(/\.workbench-shell\s*{[\s\S]*background:\s*transparent;/);
    expect(styles).toMatch(
      /\.workbench-shell \.library-pane,\s*\.workbench-shell \.detail-panel\s*{[\s\S]*border-radius:\s*0;/,
    );
    expect(styles).toMatch(
      /\.workbench-shell \.library-pane,\s*\.workbench-shell \.detail-panel\s*{[\s\S]*box-shadow:\s*none;/,
    );
    expect(styles).toMatch(/\.workbench-shell \.library-pane\s*{[\s\S]*border-right:\s*1px solid/);
    expect(styles).toMatch(/\.workbench-shell \.library-pane\s*{[\s\S]*var\(--wb-gui-opacity\)/);
    expect(styles).toMatch(/--wb-surface:\s*rgba\(20,\s*21,\s*23,\s*calc\(0\.18 \+ \(0\.82 \* var\(--wb-gui-opacity\)\)\)\);/);
    expect(styles).toMatch(/--wb-popover:\s*rgba\(24,\s*25,\s*28,\s*calc\(0\.52 \+ \(0\.48 \* var\(--wb-gui-opacity\)\)\)\);/);
    expect(styles).toMatch(/\.workbench-shell \.library-pane\s*{[\s\S]*rgba\(14,\s*15,\s*17,\s*var\(--wb-gui-opacity\)\);/);
    expect(styles).toMatch(/\.workbench-shell \.library-pane\s*{[\s\S]*backdrop-filter:\s*blur\(20px\) saturate\(115%\);/);
    expect(styles).toMatch(/\.workbench-shell \.detail-panel\s*{[\s\S]*rgba\(16,\s*18,\s*20,\s*var\(--wb-gui-opacity\)\);/);
    expect(styles).toMatch(/\.workbench-shell \.detail-panel\s*{[\s\S]*backdrop-filter:\s*blur\(24px\) saturate\(130%\);/);
    expect(styles).toMatch(/\.workbench-shell \.detail-panel::before\s*{[\s\S]*border-left:\s*1px solid/);
    expect(styles).toMatch(/\.workbench-shell \.library-card-overlay\s*{[\s\S]*right:\s*auto;/);
    expect(styles).toContain(".workbench-shell .library-card-fallback-title");
    expect(styles).toMatch(/\.workbench-shell \.library-card-fallback-title\s*{[\s\S]*-webkit-line-clamp:\s*3;/);
    expect(styles).toContain(".workbench-shell .workbench-select-button");
    expect(styles).toContain(".workbench-shell .workbench-select-menu");
    expect(styles).toContain(".workbench-shell .settings-range-row");
    expect(styles).toContain(".workbench-shell .property-range::-webkit-slider-runnable-track");
    expect(styles).toMatch(/\.workbench-shell \.property-range\s*{[\s\S]*background:\s*transparent;/);
    expect(styles).toMatch(/\.workbench-shell \.detail-inline-feedback\s*{[\s\S]*background:\s*color-mix/);
    expect(styles).not.toMatch(/\.workbench-shell \.detail-inline-feedback\s*{[\s\S]*backdrop-filter:/);
    expect(styles).not.toContain(".workbench-shell .window-drag-strip");
    expect(styles).not.toContain(".workbench-shell .detail-poster-shell");
    expect(styles).not.toContain(".workbench-shell.density-compact");
  });

  it("keeps the unified toolbar usable on narrower widths", () => {
    expect(styles).toContain(".workbench-shell .workbench-toolbar-actions");
    expect(styles).toMatch(/@media \(max-width: 860px\)[\s\S]*\.workbench-shell \.toolbar-search\s*{\s*flex-basis:\s*100%;/);
  });
});
