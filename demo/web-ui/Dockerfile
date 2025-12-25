# Build stage
FROM node:20-alpine AS builder

WORKDIR /app

# Install dependencies
COPY package*.json ./
RUN npm ci

# Copy source and build
COPY . .
RUN npm run build

# Production stage
FROM nginx:alpine

# Copy built assets
COPY --from=builder /app/dist /usr/share/nginx/html

# Copy default nginx config (will be overridden by ConfigMap in k8s)
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Create nginx user directories
RUN mkdir -p /var/cache/nginx /var/run && \
    chown -R nginx:nginx /var/cache/nginx /var/run /usr/share/nginx/html

# Switch to non-root user
USER nginx

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
