import type { ActivityEvent } from "../types/somedrive";

interface ActivityPageProps {
  events: ActivityEvent[];
  onRefresh: () => Promise<void>;
}

export function ActivityPage({ events, onRefresh }: ActivityPageProps) {
  return (
    <section class="page">
      <h2>Activity</h2>
      <article class="card">
        <div class="button-row">
          <button onClick={onRefresh}>Refresh Activity</button>
        </div>
        {events.length === 0 ? (
          <p>No activity yet. Events appear here as profiles are configured and sync agents run.</p>
        ) : (
          <div class="activity-list">
            {events.map((event) => (
              <section key={event.id} class="activity-item">
                <p>
                  <span class="pill">{event.kind}</span> {event.profileName}
                </p>
                <p>{event.message}</p>
                <p class="activity-time">{new Date(event.timestamp).toLocaleString()}</p>
              </section>
            ))}
          </div>
        )}
      </article>
    </section>
  );
}
