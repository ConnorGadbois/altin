from flask import request, jsonify
from functools import wraps
import jwt
from datetime import datetime

from .config import config

def login_required(f):
    @wraps(f)
    def login_required_decorator(*args, **kwargs):
        if 'Authorization' not in request.headers:
            return(jsonify({'message': 'Authorization header required'}), 401)

        try:
            decoded_token = jwt.decode(request.headers['Authorization'], config['jwt_secret'], algorithms=["HS256"]) 
            
        except jwt.ExpiredSignatureError:
            return(jsonify({'message': 'Invalid authorization'}), 401)

        except jwt.InvalidTokenError:
            return(jsonify({'message': 'Invalid authorization'}), 401)

        return f(*args, **kwargs)
    return(login_required_decorator)