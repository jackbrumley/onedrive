interface ToggleSwitchProps {
  id: string;
  label: string;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void | Promise<void>;
}

export function ToggleSwitch({
  id,
  label,
  description,
  checked,
  disabled = false,
  onChange,
}: ToggleSwitchProps) {
  const handleChange = (event: Event) => {
    const nextChecked = (event.currentTarget as HTMLInputElement).checked;
    void onChange(nextChecked);
  };

  return (
    <label class={`toggle-field${disabled ? " is-disabled" : ""}`} for={id}>
      <span class="toggle-copy">
        <span class="toggle-label">{label}</span>
        {description ? <span class="toggle-description">{description}</span> : null}
      </span>
      <span class="toggle-control">
        <input
          id={id}
          class="toggle-input"
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={handleChange}
        />
        <span class="toggle-track" aria-hidden="true">
          <span class="toggle-thumb" />
        </span>
      </span>
    </label>
  );
}
