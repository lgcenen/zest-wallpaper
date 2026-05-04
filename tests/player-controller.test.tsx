import { render, screen, waitFor } from "@testing-library/react";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import type { RuntimeDiagnostic } from "../src/types";

function diagnostic(code: string, summary: string): RuntimeDiagnostic {
  return {
    timestampMs: code === "initial" ? 1 : 2,
    subsystem: "native-web",
    code,
    severity: "error",
    summary,
    detail: null,
  };
}

const mocks = vi.hoisted(() => {
  let diagnosticHandler: ((diagnostics: RuntimeDiagnostic[]) => void) | undefined;
  return {
    getPlayerState: vi.fn(async () => ({ active: null, paused: false })),
    getPlayerDiagnostics: vi.fn(async () => [
      {
        timestampMs: 1,
        subsystem: "native-web",
        code: "initial",
        severity: "error",
        summary: "Initial failure",
        detail: null,
      },
    ]),
    onPlayerLoad: vi.fn(async () => () => undefined),
    onPlayerPause: vi.fn(async () => () => undefined),
    onPlayerUpdate: vi.fn(async () => () => undefined),
    onPlayerDiagnostics: vi.fn(async (handler: (diagnostics: RuntimeDiagnostic[]) => void) => {
      diagnosticHandler = handler;
      return () => {
        diagnosticHandler = undefined;
      };
    }),
    emitDiagnostics: (diagnostics: RuntimeDiagnostic[]) => {
      diagnosticHandler?.(diagnostics);
    },
  };
});

vi.mock("../src/gateway", () => ({
  getPlayerState: mocks.getPlayerState,
  getPlayerDiagnostics: mocks.getPlayerDiagnostics,
  onPlayerLoad: mocks.onPlayerLoad,
  onPlayerPause: mocks.onPlayerPause,
  onPlayerUpdate: mocks.onPlayerUpdate,
  onPlayerDiagnostics: mocks.onPlayerDiagnostics,
}));

import { usePlayerController } from "../src/state/player-controller";

function PlayerStateProbe() {
  const state = usePlayerController();
  return (
    <output aria-label="diagnostics">
      {state.diagnostics.map((entry) => entry.summary).join(",")}
    </output>
  );
}

describe("player controller diagnostics", () => {
  it("loads and subscribes to runtime diagnostics", async () => {
    render(<PlayerStateProbe />);

    await waitFor(() => {
      expect(screen.getByLabelText("diagnostics").textContent).toBe("Initial failure");
    });

    act(() => {
      mocks.emitDiagnostics([diagnostic("next", "Bridge sync failed")]);
    });

    expect(screen.getByLabelText("diagnostics").textContent).toBe("Bridge sync failed");
  });
});
