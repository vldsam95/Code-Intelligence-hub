from fastapi import FastAPI

from services import health_payload

app = FastAPI()


@app.get("/health")
def health() -> dict[str, bool]:
    return health_payload()

