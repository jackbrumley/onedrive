import { IconChevronLeft } from "@tabler/icons-preact";
import { ToggleSwitch } from "../components/ui/ToggleSwitch";

interface SettingsPageProps {
  autostartEnabled: boolean;
  onToggleAutostart: (enabled: boolean) => Promise<void>;
  onGoDebug: () => void;
  onBack: () => void;
}

export function SettingsPage({ autostartEnabled, onToggleAutostart, onGoDebug, onBack }: SettingsPageProps) {
  return (
    <section class="page">
      <div class="page-header">
        <a
          class="page-header-back-link"
          href="#/accounts"
          onClick={(event) => {
            event.preventDefault();
            onBack();
          }}
          aria-label="Back to accounts"
          title="Back to accounts"
        >
          <IconChevronLeft size={36} stroke={2.2} />
        </a>
        <h2>Settings</h2>
      </div>

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
