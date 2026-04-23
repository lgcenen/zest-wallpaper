import WorkbenchApp from "./app-shell/WorkbenchApp";
import { PlayerAppShell } from "./app-shell/player-runtime";

export default function App() {
  if (window.__WALLPAPER_PLAYER__) {
    return <PlayerAppShell />;
  }

  return <WorkbenchApp />;
}
