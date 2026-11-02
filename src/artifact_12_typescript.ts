import * as http from "http";

const HEALTH_PORT = process.env.PORT || 8080;

const server = http.createServer((req, res) => {
  if (req.url === "/health") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({
      status: "healthy",
      uptime: process.uptime(),
      timestamp: new Date().toISOString(),
      service: "creator-nexus-event-dispatcher"
    }));
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(HEALTH_PORT, () => {
  logger.info(`Health check listening on port ${HEALTH_PORT}`);
});