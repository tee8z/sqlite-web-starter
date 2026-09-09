// One request asks the server to launch concurrent write calls on isolated databases.
(() => {
  const button = document.getElementById("run-contention");
  const status = document.getElementById("contention-status");
  const results = document.getElementById("contention-results");
  if (!button || !status || !results || !window.fetch) return;

  const elapsed = (milliseconds) => milliseconds < 1 ? "<1" : String(Math.round(milliseconds));
  const isTime = (value) => Number.isFinite(value) && value >= 0;
  const isCount = (value) => Number.isSafeInteger(value) && value >= 0;
  const validPath = (path, writers) => path &&
    isCount(path.committed) && isCount(path.busy) && isCount(path.final_value) &&
    path.committed + path.busy === writers && isTime(path.elapsed_ms) &&
    Array.isArray(path.requests) && path.requests.length === writers &&
    path.requests.every((request, index) => request.request === index + 1 &&
      (request.outcome === "committed" || request.outcome === "busy") &&
      (request.outcome === "busy" ? request.value === null : isCount(request.value)) &&
      isTime(request.started_ms) && isTime(request.elapsed_ms));

  function renderPath(name, path, writers, scale) {
    document.getElementById(name + "-committed").textContent = path.committed + " / " + writers;
    document.getElementById(name + "-busy").textContent = String(path.busy);
    document.getElementById(name + "-value").textContent = String(path.final_value);
    document.getElementById(name + "-duration").textContent = "Burst completed in " + elapsed(path.elapsed_ms) + " ms";
    const list = document.getElementById(name + "-requests");
    list.textContent = "";

    path.requests.forEach((request) => {
      const committed = request.outcome === "committed";
      const row = document.createElement("li");
      row.className = "request-row " + (committed ? "request-committed" : "request-busy");
      const description = "Request " + request.request + ": " +
        (committed ? "committed counter value " + request.value : "SQLITE_BUSY; did not commit") +
        "; elapsed " + elapsed(request.elapsed_ms) + " milliseconds including wait.";
      row.title = description;

      const accessible = document.createElement("span");
      accessible.className = "visually-hidden";
      accessible.textContent = description;
      row.appendChild(accessible);

      const label = document.createElement("span");
      label.className = "request-number";
      label.setAttribute("aria-hidden", "true");
      label.textContent = "#" + request.request;
      row.appendChild(label);

      const track = document.createElement("span");
      track.className = "timing-track";
      track.setAttribute("aria-hidden", "true");
      const bar = document.createElement("span");
      bar.className = "timing-bar";
      bar.style.left = (request.started_ms / scale * 100) + "%";
      bar.style.width = (request.elapsed_ms / scale * 100) + "%";
      track.appendChild(bar);
      row.appendChild(track);

      const outcome = document.createElement("span");
      outcome.className = "request-outcome";
      outcome.setAttribute("aria-hidden", "true");
      outcome.textContent = (committed ? "Saved" : "SQLITE_BUSY") + " · " + elapsed(request.elapsed_ms) + " ms";
      row.appendChild(outcome);
      list.appendChild(row);
    });
  }

  button.disabled = false;
  status.textContent = "Compare both paths using fresh counters. Your saved counter stays unchanged.";
  button.addEventListener("click", async () => {
    if (button.disabled) return;
    button.disabled = true;
    button.setAttribute("aria-busy", "true");
    results.hidden = true;
    status.textContent = "Running two bursts of 12 concurrent write calls on the server…";
    const controller = window.AbortController ? new AbortController() : null;
    const timeout = controller ? window.setTimeout(() => controller.abort(), 15000) : null;
    let failureMessage = "Could not complete the comparison. Try running it again. Your saved counter is unchanged.";
    try {
      const response = await fetch("/contention", {
        method: "POST",
        headers: { Accept: "application/json" },
        signal: controller ? controller.signal : undefined,
      });
      if (response.status === 429) {
        failureMessage = "A comparison is already running. Wait a moment, then try again. Your saved counter is unchanged.";
      }
      if (!response.ok) throw new Error("The server could not complete the comparison.");
      const result = await response.json();
      if (!isCount(result.writers) || result.writers === 0 ||
          !validPath(result.direct, result.writers) || !validPath(result.queued, result.writers)) {
        throw new Error("Invalid comparison response.");
      }
      const requests = result.direct.requests.concat(result.queued.requests);
      const scale = Math.max(1, ...requests.map((request) => request.started_ms + request.elapsed_ms));
      renderPath("direct", result.direct, result.writers, scale);
      renderPath("queued", result.queued, result.writers, scale);
      document.getElementById("timing-note").textContent =
        "Each row is one call. Both charts use the same 0–" + elapsed(scale) +
        " ms scale. Bars show measured request time, including queue wait; tiny bars are shown as a visible mark.";
      document.getElementById("contention-takeaway").textContent = result.direct.busy > 0 ?
        "The independent writers collided with SQLite’s write lock. The channel made callers wait their turn; the longer request times show that queue wait." :
        "The independent calls did not encounter a busy lock in this run. Scheduling can vary; run again to observe contention. The channel still routes every call through one writer.";
      results.hidden = false;
      status.textContent = "Complete. Independent writers: " + result.direct.committed + " committed, " +
        result.direct.busy + " busy. Channel: " + result.queued.committed + " committed, " +
        result.queued.busy + " busy.";
    } catch (error) {
      status.textContent = error.name === "AbortError" ?
        "The comparison timed out. Wait a moment, then try again. Your saved counter is unchanged." : failureMessage;
    } finally {
      if (timeout !== null) window.clearTimeout(timeout);
      button.disabled = false;
      button.removeAttribute("aria-busy");
    }
  });
})();
