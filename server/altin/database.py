import peewee
import uuid
import json
from datetime import datetime
import hashlib

from .config import config

if config['database'] == 'sqlite':
    db = peewee.SqliteDatabase(config['db_path'], pragmas={'journal_mode': 'wal'})

if config['database'] == 'postgres':
    db = peewee.PostgresqlDatabase(
        config['db_db'],
        host=config['db_host'],
        port=int(config['db_port']) if config['db_port'] not in (None, 'None') else 5432,
        user=config['db_user'],
        password=config['db_password']
    )

class Agent(peewee.Model):
    id = peewee.CharField(null=False, unique=True, default=uuid.uuid4)
    implant_id = peewee.CharField(null=False)
    ip = peewee.CharField(null=False)
    os = peewee.CharField(null=False)
    commands = peewee.TextField(null=False)
    tags = peewee.TextField(null=False, default='[]')
    last_checkin = peewee.DateTimeField(null=False, default=datetime.now)
    
    def serialize(self):
        return({
            'id': self.id,
            'implant_id': self.implant_id,
            'ip': self.ip,
            'os': self.os,
            'commands': json.loads(self.commands),
            'tags': json.loads(self.tags),
            'last_checkin': self.last_checkin
        })

    class Meta:
        database = db
        db_table = 'agents'

class Task(peewee.Model):
    id = peewee.CharField(null=False, unique=True, default=uuid.uuid4)
    agent = peewee.ForeignKeyField(Agent, 'id', null=False, on_delete='CASCADE')
    task = peewee.CharField(null=False)
    args = peewee.TextField(null=False, default='[]')
    sent = peewee.BooleanField(null=False, default=False)
    completed = peewee.BooleanField(null=False, default=False)

    def serialize(self):
        return({
            'id': self.id,
            'agent': str(self.agent),
            'task': self.task,
            'args': json.loads(self.args),
            'sent': self.sent,
            'completed': self.completed
        })

    class Meta:
        database = db
        db_table = 'tasks'

class TaskResult(peewee.Model):
    id = peewee.CharField(null=False, unique=True, default=uuid.uuid4)
    task = peewee.ForeignKeyField(Task, 'id', null=False, on_delete='CASCADE')
    result = peewee.TextField(null=True)
    timestamp = peewee.DateTimeField(null=False, default=datetime.now)

    def serialize(self):
        return({
            'id': self.id,
            'task': str(self.task),
            'result': self.result,
            'timestamp': str(self.timestamp)
        })

    class Meta:
        database = db
        db_table = 'task_results'

class Key(peewee.Model):
    id = peewee.CharField(null=False, unique=True, default=uuid.uuid4)
    name = peewee.CharField(null=False)
    key = peewee.CharField(null=False)

    def serialize(self):
        return({
            'id': self.id,
            'name': self.name,
            'key': self.key
        })

    class Meta:
        database = db
        db_table = 'keys'

class User(peewee.Model):
    id = peewee.CharField(null=False, unique=True, default=uuid.uuid4)
    username = peewee.CharField(null=False)
    password = peewee.CharField(null=False)
    admin = peewee.BooleanField(null=False, default=False)

    def serialize(self):
        return({
            'id': self.id,
            'username': self.username,
            'admin': self.admin
        })

    class Meta:
        database = db
        db_table = 'user'

def init_db() -> None:
    db.create_tables([Agent, Task, TaskResult, Key, User])

    # Default user check
    if not User.select().where(User.username == 'admin').exists():
        User.create(username='admin', password=hashlib.sha512('admin'.encode('utf-8')).hexdigest(), admin=1)
