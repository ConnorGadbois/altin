FROM nginx:alpine

RUN rm -rf /usr/share/nginx/html/*

COPY --exclude=Dockerfile . /usr/share/nginx/html

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
