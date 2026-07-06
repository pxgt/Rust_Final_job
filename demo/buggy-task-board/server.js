const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");

const publicRoot = path.join(__dirname, "public");
const portArgumentIndex = process.argv.indexOf("--port");
const port =
  portArgumentIndex >= 0
    ? Number(process.argv[portArgumentIndex + 1])
    : Number(process.env.PORT || 4173);

const staticRoutes = new Map([
  ["/", ["index.html", "text/html; charset=utf-8"]],
  ["/index.html", ["index.html", "text/html; charset=utf-8"]],
  ["/styles.css", ["styles.css", "text/css; charset=utf-8"]],
  ["/app.js", ["app.js", "text/javascript; charset=utf-8"]],
]);

function send(response, status, contentType, body) {
  const payload = Buffer.from(body);
  response.writeHead(status, {
    "Content-Type": contentType,
    "Content-Length": payload.length,
    "Cache-Control": "no-store",
  });
  response.end(payload);
}

function sendJson(response, status, value) {
  send(
    response,
    status,
    "application/json; charset=utf-8",
    JSON.stringify(value),
  );
}

const server = http.createServer((request, response) => {
  const url = new URL(request.url, `http://${request.headers.host}`);

  if (url.pathname === "/health") {
    sendJson(response, 200, { status: "ok", demo: "buggy-task-board" });
    return;
  }

  if (url.pathname === "/api/tasks") {
    sendJson(response, 500, {
      code: "TASK_STORE_UNAVAILABLE",
      message: "The task store is temporarily unavailable.",
    });
    return;
  }

  const route = staticRoutes.get(url.pathname);
  if (!route) {
    sendJson(response, 404, {
      code: "NOT_FOUND",
      message: `No route exists for ${url.pathname}.`,
    });
    return;
  }

  const [fileName, contentType] = route;
  fs.readFile(path.join(publicRoot, fileName), (error, contents) => {
    if (error) {
      sendJson(response, 500, {
        code: "STATIC_FILE_ERROR",
        message: error.message,
      });
      return;
    }
    send(response, 200, contentType, contents);
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`Buggy Task Board listening on http://127.0.0.1:${port}`);
});
