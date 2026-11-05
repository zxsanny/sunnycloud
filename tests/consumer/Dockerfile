# Black-box test consumer (tests/compose.test.yml). No SUT sources or internal imports: only the wire.
FROM python:3.13-slim@sha256:d0a9e379c0c588befb0e427db17ca9731751d281f32c6cb1a90a1277b680bdad

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PYTHONPATH=/app/plugin

WORKDIR /app
COPY requirements.txt ./
RUN pip install --verbose -r requirements.txt

COPY e2e_report.py ./plugin/
COPY pytest.ini conftest.py ./
COPY selftests ./selftests

USER 10004:10004
CMD ["pytest", "-v", "--tb=short", "-p", "e2e_report"]
