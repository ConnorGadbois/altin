FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

WORKDIR /app

COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt gunicorn==23.0.0

COPY altin/ ./altin/

RUN useradd --create-home --uid 10001 altin && chown -R altin:altin /app
USER altin

CMD ["sh", "-c", "exec gunicorn --bind 0.0.0.0:${ALTIN_PORT:-3000} --workers 1 --threads 4 --access-logfile - --error-logfile - altin:app"]
