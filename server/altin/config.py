import os
from dotenv import load_dotenv

config = {}

def load_config() -> None:
    global config

    load_dotenv()

    try:
        config = {
            'port': int(os.environ.get('ALTIN_PORT', default=3030)),
            'database': str(os.environ.get('ALTIN_DATABASE', default='sqlite')),
            'db_path': str(os.environ.get('ALTIN_SQLITE_PATH', default='altin.db')),
            'db_db': str(os.environ.get('ALTIN_PGSQL_DATABASE', default=None)),
            'db_host': str(os.environ.get('ALTIN_PGSQL_HOST', default=None)),
            'db_port': str(os.environ.get('ALTIN_PGSQL_PORT', default=None)),
            'db_user': str(os.environ.get('ALTIN_PGSQL_USER', default=None)),
            'db_password': str(os.environ.get('ALTIN_PGSQL_PASSWORD', default=None)),
            'jwt_secret': str(os.environ.get('ALTIN_JWT_SECRET', default=None))
        }

    except Exception as e:
        print(f'Failed to load config: {e}')
        quit(1)

def validate_config() -> None:
    global config

    valid = True
    message = ''

    if config['database'] not in ['sqlite', 'postgres']:
        message += 'ALTIN_DATABASE must be `sqlite` or `postgres`\n'
        valid = False

    if not config['jwt_secret']:
        message += 'ALTIN_JWT_SECRET must be set\n'
        valid = False
        
    if not valid:
        print('Invalid config!')
        print(message)
        quit(1)