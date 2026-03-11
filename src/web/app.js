(() => {
  const $ = (id) => document.getElementById(id);

  const connStatus = $("connStatus");
  const lastEvent = $("lastEvent");
  const eventsEl = $("events");

  let ws = null;

  const now = () => new Date().toISOString();

  function addEvent(kind, obj, extraNode) {
    lastEvent.textContent = `${now()} ${kind}`;
    const div = document.createElement("div");
    div.className = "event";
    const meta = document.createElement("div");
    meta.className = "meta";
    meta.textContent = `${now()}  ${kind}`;
    const pre = document.createElement("pre");
    pre.style.margin = "0";
    pre.textContent = typeof obj === "string" ? obj : JSON.stringify(obj, null, 2);
    div.appendChild(meta);
    if (extraNode) div.appendChild(extraNode);
    div.appendChild(pre);
    eventsEl.prepend(div);
  }

  function setStatus(s) {
    connStatus.textContent = s;
  }

  function sendEnv(data) {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      addEvent("error", "WS not connected");
      return;
    }
    ws.send(JSON.stringify({ v: 1, data }));
  }

  function connect() {
    const url = $("wsUrl").value.trim();
    if (!url) return;
    if (ws) ws.close();
    ws = new WebSocket(url);
    setStatus("connecting...");

    ws.onopen = () => {
      setStatus("connected");
      const token = $("authToken").value.trim();
      if (token) {
        sendEnv({ type: "Auth", data: { token } });
      }
    };
    ws.onclose = () => setStatus("disconnected");
    ws.onerror = () => setStatus("error");
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data);
        const t = msg?.data?.type;
        if (t === "NeedsApproval") {
          const approval_id = msg.data.data.approval_id;
          const summary = msg.data.data.summary;
          const actions = document.createElement("div");
          actions.style.display = "flex";
          actions.style.gap = "10px";
          actions.style.margin = "6px 0 10px";

          const btnA = document.createElement("button");
          btnA.textContent = "Approve";
          btnA.onclick = () =>
            sendEnv({
              type: "ApprovalResponse",
              data: { approval_id, approve: true },
            });
          const btnD = document.createElement("button");
          btnD.textContent = "Deny";
          btnD.onclick = () =>
            sendEnv({
              type: "ApprovalResponse",
              data: { approval_id, approve: false },
            });
          actions.appendChild(btnA);
          actions.appendChild(btnD);

          addEvent("NeedsApproval", { approval_id, summary }, actions);
        } else {
          addEvent("inbound", msg);
        }
      } catch (e) {
        addEvent("inbound(raw)", ev.data);
      }
    };
  }

  $("btnConnect").onclick = connect;
  $("btnSubscribe").onclick = () => {
    const topics = $("topics")
      .value.split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    sendEnv({ type: "Subscribe", data: { topics } });
  };
  $("btnClear").onclick = () => {
    eventsEl.innerHTML = "";
    lastEvent.textContent = "—";
  };
  $("btnSend").onclick = () => {
    const session_id = $("sessionId").value.trim() || "cli";
    const input = $("msgInput").value;
    sendEnv({ type: "SendMessage", data: { session_id, input } });
  };
  $("btnOrchestrate").onclick = () => {
    const session_id = $("sessionId").value.trim() || "cli";
    const goal = $("goalInput").value.trim();
    const agents = $("agents")
      .value.split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (!goal) return;
    sendEnv({ type: "Orchestrate", data: { session_id, goal, agents } });
  };

  // Auto-connect for convenience.
  connect();
})();

