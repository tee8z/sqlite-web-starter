// Enhance the server-rendered form. Its normal POST still works without this script.
(() => {
  const form = document.querySelector('form[action="/increment"]');
  const counter = document.getElementById("counter-value");
  const status = document.getElementById("counter-status");
  const button = form && form.querySelector('button[type="submit"]');
  if (!form || !counter || !status || !button || !window.fetch) return;

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (button.disabled) return;
    button.disabled = true;
    form.setAttribute("aria-busy", "true");
    status.textContent = "Saving…";

    try {
      const response = await fetch("/counter", {
        method: "POST",
        headers: { Accept: "application/json" },
      });
      if (!response.ok) throw new Error("The server did not confirm the write.");
      const result = await response.json();
      if (!Number.isSafeInteger(result.value)) throw new Error("Invalid counter response.");
      counter.value = String(result.value);
      status.textContent = "Saved.";
    } catch {
      // A failed reply can follow a committed write. Do not retry automatically.
      status.textContent = "Could not confirm the update. Reload the page before trying again.";
    } finally {
      button.disabled = false;
      form.removeAttribute("aria-busy");
    }
  });
})();
