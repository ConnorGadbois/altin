import re
import random
import string
import base64

def random_name(length=8):
    return ''.join(random.choices(string.ascii_letters, k=length))

def tokenize_php(code):
    token_pattern = re.compile(
        r"""
        ("(?:\\.|[^"])*") |          # double quoted string
        ('(?:\\.|[^'])*') |          # single quoted string
        (//[^\n]*\n?) |              # single-line comment
        (/\*.*?\*/) |                # multi-line comment
        (\$[a-zA-Z_]\w*) |           # variable
        ([a-zA-Z_]\w*) |             # identifier
        (\s+) |                      # whitespace
        (.)                          # everything else
        """,
        re.VERBOSE | re.DOTALL
    )

    tokens = []
    for match in token_pattern.finditer(code):
        tokens.append(match.group(0))
    return tokens


def obfuscate_php(code: str, wrap_eval=False, one_liner=False) -> str:
    tokens = tokenize_php(code)

    var_map = {}
    func_map = {}
    superglobals = {
        "$_GET", "$_POST", "$_REQUEST", "$_SERVER",
        "$_SESSION", "$_COOKIE", "$_FILES", "$_ENV", "$GLOBALS"
    }

    output = []
    next_is_function_name = False
    i = 0

    while i < len(tokens):
        token = tokens[i]

        # Preserve strings and comments
        if token.startswith(("'", '"', "//", "/*")):
            output.append(token)
            i += 1
            continue

        # Detect function keyword
        if token == "function":
            next_is_function_name = True
            output.append(token)
            i += 1
            continue

        # Rename function declaration
        if next_is_function_name and re.match(r'[a-zA-Z_]\w*', token):
            if token not in func_map:
                func_map[token] = random_name()
            output.append(func_map[token])
            next_is_function_name = False
            i += 1
            continue

        # Handle variables
        if re.match(r'\$[a-zA-Z_]\w*', token):

            original_var = token

            if original_var in superglobals:
                output.append(original_var)
                i += 1
                continue

            if original_var not in var_map:
                var_map[original_var] = "$" + random_name()

            new_var = var_map[original_var]

            # Detect assignment to string literal
            j = i + 1
            while j < len(tokens) and tokens[j].isspace():
                j += 1

            if j < len(tokens) and tokens[j] == "=":
                k = j + 1
                while k < len(tokens) and tokens[k].isspace():
                    k += 1

                if (
                    k < len(tokens)
                    and (tokens[k].startswith('"') or tokens[k].startswith("'"))
                ):
                    string_literal = tokens[k]
                    unquoted = string_literal[1:-1]
                    encoded = base64.b64encode(unquoted.encode()).decode()

                    output.append(new_var)
                    output.append("=")
                    output.append(f'base64_decode("{encoded}")')

                    i = k + 1
                    continue

            output.append(new_var)
            i += 1
            continue

        # Rename function calls
        if token in func_map:
            output.append(func_map[token])
            i += 1
            continue

        output.append(token)
        i += 1

    result = ''.join(output)

    # -------- MAKE ONE LINER --------
    if one_liner:
        # Collapse all whitespace to single spaces
        result = re.sub(r'\s+', ' ', result)

        # Remove spaces around common operators/punctuation
        result = re.sub(r'\s*([{}();,=+\-*/<>])\s*', r'\1', result)

        result = result.strip()

    # Optional full script base64 wrap
    if wrap_eval:
        encoded = base64.b64encode(result.encode()).decode()
        result = f'eval(base64_decode("{encoded}"));'

    return result

payload = open('notka.php', 'r')
obfuscated_payload = obfuscate_php(payload.read(), True, True)
payload.close()

output = open('payload.php', 'w')
output.write(obfuscated_payload)
output.close()