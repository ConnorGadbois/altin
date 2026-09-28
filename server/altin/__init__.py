from flask import Flask, request
from flask_cors import CORS

from .config import load_config, validate_config

load_config()
validate_config()

from .config import config

from .database import init_db
init_db()

from .c2_routes import c2_routes
from .management_routes import management_routes

app = Flask(__name__)

CORS(app, resources={r'/api/*': {'origins': '*'}})

app.register_blueprint(c2_routes)
app.register_blueprint(management_routes)
