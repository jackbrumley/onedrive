import type { ActivityEvent } from "../../types/somedrive";

interface AccountActivityPanelProps {
  events: ActivityEvent[];
}

export function AccountActivityPanel({ events }: AccountActivityPanelProps) {
  return (
    <article class="card">
      <h3>Account Activity</h3>
      {events.length === 0 ? (
        <p>No activity yet for this account.</p>
      ) : (
        <div class="activity-list">
          {events.map((event) => (
            <section key={event.id} class="activity-item">
              <p>
                <span class="pill">{event.kind}</span> {event.message}
              </p>
              <p class="activity-time">{new Date(event.timestamp).toLocaleString()}</p>
            </section>
          ))}
        </div>
      )}
    </article>
  );
}
