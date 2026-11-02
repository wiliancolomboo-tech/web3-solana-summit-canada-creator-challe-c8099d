import * as http from "http";

const PORT = process.env.PORT || 8080;

export function initHealthCheckServer() {
  const server = http.createServer((req, res) => {
    if (req.url === "/health" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ status: "healthy", timestamp: Date.now() }));
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  server.listen(PORT, () => {
    console.log(`[HealthCheck] Service telemetry listening on port ${PORT}`);
  });
}