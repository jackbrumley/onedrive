import { ToggleSwitch } from "../components/ui/ToggleSwitch";

interface SettingsPageProps {
  autostartEnabled: boolean;
  onToggleAutostart: (enabled: boolean) => Promise<void>;
  onGoDebug: () => void;
}

export function SettingsPage({ autostartEnabled, onToggleAutostart, onGoDebug }: SettingsPageProps) {
  return (
    <section class="page">
      <h2 class="settings-title">Settings</h2>

      <article class="card">
        <h3>General</h3>
        <p>Set how SomeDrive starts up on this device.</p>
        <div class="settings-list">
          <ToggleSwitch
            id="autostart-toggle"
            label="Start SomeDrive when I log in"
            description="Available on Linux, Windows, and macOS desktop builds."
            checked={autostartEnabled}
            onChange={onToggleAutostart}
          />
        </div>
      </article>

      <article class="card">
        <h3>Developer</h3>
        <p>Debug and diagnostics tools are grouped separately from regular settings.</p>
        <div class="button-row">
          <button onClick={onGoDebug}>Open Debug Tools</button>
        </div>
      </article>
    </section>
  );
}
