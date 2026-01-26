# Cedar Agent API Documentation

## Overview

Cedar Agent is an HTTP server that efficiently manages a policy store and a data store, providing seamless integration with [Cedar](https://www.cedarpolicy.com/en), a language for defining permissions as policies. The API allows you to manage policies, data, schemas, and perform authorization checks using the Cedar policy engine.

**Base URL**: `http://localhost:8180/v1`
**API Documentation**: Available at `/swagger-ui/` and `/rapidoc/` endpoints

## Authentication

All API endpoints (except health check) require authentication using an API key in the `Authorization` header:

```
Authorization: <your-api-key>
```

The API key can be configured via:
- `CEDAR_AGENT_AUTHENTICATION` environment variable
- `--authentication` or `-a` command line argument

## Response Format

All responses are returned in JSON format with appropriate HTTP status codes.

## Error Handling

The API uses standard HTTP status codes and returns error details in the response body:

- `400 Bad Request`: Invalid request parameters or data
- `404 Not Found`: Resource not found
- `409 Conflict`: Duplicate resource
- `500 Internal Server Error`: Server error

Error response format:
```json
{
  "error": "Error description",
  "details": "Additional error details"
}
```

## API Endpoints

### Invalidation Management

These endpoints manage which database targets cedar-agent will invalidate (flush authorization caches) on policy/schema/data mutations.

- `GET /invalidation/targets`: list targets
- `PUT /invalidation/targets`: replace the full target list

DB-origin entity sync (used by database extensions) should send:
- `X-Cedar-Write-Origin: db-entity-sync`

When this header is present on entity sync requests, cedar-agent skips invalidation to avoid double-flushing and added DDL latency.

### Health Check

#### GET /

Check if the server is running and healthy.

**Response**: `204 No Content`

### Policy Management

#### GET /policies

Retrieve all stored policies.

**Authentication**: Required

**Response**:
```json
[
  {
    "id": "policy-1",
    "effect": "permit",
    "principal": {
      "op": "==",
      "entity": {
        "type": "User",
        "id": "alice"
      }
    },
    "action": {
      "op": "==",
      "entity": {
        "type": "Action",
        "id": "view"
      }
    },
    "resource": {
      "op": "==",
      "entity": {
        "type": "Document",
        "id": "doc1"
      }
    },
    "conditions": []
  }
]
```

#### GET /policies/{id}

Retrieve a specific policy by ID.

**Authentication**: Required

**Path Parameters**:
- `id` (string): Policy ID

**Response**: Policy object (same format as above)

**Errors**:
- `404 Not Found`: Policy not found

#### POST /policies

Create a new policy.

**Authentication**: Required

**Request Body**:
```json
{
  "id": "policy-1",
  "effect": "permit",
  "principal": {
    "op": "==",
    "entity": {
      "type": "User",
      "id": "alice"
    }
  },
  "action": {
    "op": "==",
    "entity": {
      "type": "Action",
      "id": "view"
    }
  },
  "resource": {
    "op": "==",
    "entity": {
      "type": "Document",
      "id": "doc1"
    }
  },
  "conditions": []
}
```

**Response**: Created policy object

**Errors**:
- `400 Bad Request`: Invalid policy format or schema validation failure
- `409 Conflict`: Policy with this ID already exists

#### PUT /policies

Update multiple policies (bulk operation).

**Authentication**: Required

**Request Body**: Array of policy objects

**Response**: Array of updated policy objects

**Errors**:
- `400 Bad Request`: Invalid policy format or schema validation failure

#### PUT /policies/{id}

Update a specific policy by ID.

**Authentication**: Required

**Path Parameters**:
- `id` (string): Policy ID

**Request Body**:
```json
{
  "effect": "permit",
  "principal": {
    "op": "==",
    "entity": {
      "type": "User",
      "id": "bob"
    }
  },
  "action": {
    "op": "==",
    "entity": {
      "type": "Action",
      "id": "edit"
    }
  },
  "resource": {
    "op": "==",
    "entity": {
      "type": "Document",
      "id": "doc1"
    }
  },
  "conditions": []
}
```

**Response**: Updated policy object

**Errors**:
- `400 Bad Request`: Invalid policy format or schema validation failure

#### DELETE /policies/{id}

Delete a specific policy by ID.

**Authentication**: Required

**Path Parameters**:
- `id` (string): Policy ID

**Response**: `204 No Content`

**Errors**:
- `404 Not Found`: Policy not found

### Data/Entity Management

#### GET /data

Retrieve all stored entities.

**Authentication**: Required

**Response**:
```json
[
  {
    "uid": {
      "type": "User",
      "id": "alice"
    },
    "attrs": {
      "department": "engineering",
      "clearance_level": "high"
    },
    "parents": [
      {
        "type": "Role",
        "id": "admin"
      }
    ]
  }
]
```

#### PUT /data

Update/replace all entities (bulk operation).

**Authentication**: Required

**Request Body**: Array of entity objects

**Response**: Array of updated entity objects

**Errors**:
- `400 Bad Request`: Invalid entity format or schema validation failure

#### DELETE /data

Delete all entities.

**Authentication**: Required

**Response**: `204 No Content`

#### PUT /data/entity

Add a new entity.

**Authentication**: Required

**Request Body**:
```json
{
  "entity_id": "alice",
  "entity_type": "User"
}
```

**Response**: Array of all entities including the new one

**Errors**:
- `400 Bad Request`: Invalid entity format or schema validation failure
- `409 Conflict`: Entity with this ID already exists

#### PUT /data/single

Add a single entity entry (alternative endpoint).

**Authentication**: Required

**Request Body**: Array with single entity object

**Response**: Array of all entities

**Errors**:
- `400 Bad Request`: Invalid entity format or schema validation failure

#### PUT /data/single/{entity_id}

Update a single entity by ID.

**Authentication**: Required

**Path Parameters**:
- `entity_id` (string): Entity ID

**Request Body**: Array with single entity object

**Response**: Updated entity object

**Errors**:
- `400 Bad Request`: Invalid entity format or schema validation failure

#### DELETE /data/single/{entity_id}

Delete a single entity by ID.

**Authentication**: Required

**Path Parameters**:
- `entity_id` (string): Entity ID

**Response**: `204 No Content`

**Errors**:
- `400 Bad Request`: Schema validation failure

#### PUT /data/attribute

Update an attribute of a specific entity.

**Authentication**: Required

**Request Body**:
```json
{
  "entity_id": "alice",
  "attribute_name": "department",
  "attribute_value": "marketing"
}
```

**Response**: Updated entity object

**Errors**:
- `400 Bad Request`: Entity not found or invalid attribute

#### DELETE /data/attribute

Delete an attribute from a specific entity.

**Authentication**: Required

**Request Body**:
```json
{
  "entity_id": "alice",
  "attribute_name": "department"
}
```

**Response**: Updated entity object

**Errors**:
- `400 Bad Request`: Entity not found or attribute doesn't exist

### Schema Management

#### GET /schema

Retrieve the current schema.

**Authentication**: Required

**Response**:
```json
{
  "": {
    "entityTypes": {
      "User": {
        "shape": {
          "type": "Record",
          "attributes": {
            "department": {
              "type": "String",
              "required": false
            }
          }
        },
        "memberOfTypes": ["Role"]
      }
    },
    "actions": {
      "view": {
        "appliesTo": {
          "principalTypes": ["User"],
          "resourceTypes": ["Document"]
        }
      }
    }
  }
}
```

#### PUT /schema

Update the entire schema.

**Authentication**: Required

**Request Body**: Schema object

**Response**: Updated schema object

**Errors**:
- `400 Bad Request`: Invalid schema format or conflicts with existing policies/data

#### DELETE /schema

Delete the current schema.

**Authentication**: Required

**Response**: `204 No Content`

#### POST /schema/user/attribute

Add an attribute to the User entity type.

**Authentication**: Required

**Request Body**:
```json
{
  "name": "department",
  "type": "String",
  "required": false
}
```

**Response**: Updated schema object

**Errors**:
- `400 Bad Request`: Invalid attribute format or conflicts with existing policies/data

#### POST /schema/resource/attribute

Add an attribute to the Table/Resource entity type.

**Authentication**: Required

**Request Body**:
```json
{
  "name": "classification",
  "type": "String",
  "required": false
}
```

**Response**: Updated schema object

**Errors**:
- `400 Bad Request`: Invalid attribute format or conflicts with existing policies/data

#### DELETE /schema/user/attribute/{attr_name}

Delete an attribute from the User entity type.

**Authentication**: Required

**Path Parameters**:
- `attr_name` (string): Attribute name

**Response**: `204 No Content`

**Errors**:
- `400 Bad Request`: Attribute not found or conflicts with existing policies/data

#### DELETE /schema/resource/attribute/{attr_name}

Delete an attribute from the Table/Resource entity type.

**Authentication**: Required

**Path Parameters**:
- `attr_name` (string): Attribute name

**Response**: `204 No Content`

**Errors**:
- `400 Bad Request`: Attribute not found or conflicts with existing policies/data

### Authorization

#### POST /is_authorized

Perform an authorization check using stored policies and data.

**Authentication**: Required

**Request Body**:
```json
{
  "principal": {
    "type": "User",
    "id": "alice"
  },
  "action": {
    "type": "Action",
    "id": "view"
  },
  "resource": {
    "type": "Document",
    "id": "doc1"
  },
  "context": {}
}
```

**Response**:
```json
{
  "decision": "Allow",
  "diagnostics": {
    "reason": ["policy-id-that-allowed"],
    "errors": []
  }
}
```

**Errors**:
- `400 Bad Request`: Invalid authorization request format

## Data Formats

### Entity Format

Entities in Cedar follow this structure:

```json
{
  "uid": {
    "type": "EntityType",
    "id": "entity-id"
  },
  "attrs": {
    "attribute_name": "attribute_value"
  },
  "parents": [
    {
      "type": "ParentType",
      "id": "parent-id"
    }
  ]
}
```

### Policy Format

Policies in Cedar follow this structure:

```json
{
  "id": "policy-id",
  "effect": "permit|deny",
  "principal": {
    "op": "==|!=|in|not in",
    "entity": {
      "type": "EntityType",
      "id": "entity-id"
    }
  },
  "action": {
    "op": "==|!=|in|not in",
    "entity": {
      "type": "Action",
      "id": "action-id"
    }
  },
  "resource": {
    "op": "==|!=|in|not in",
    "entity": {
      "type": "EntityType",
      "id": "entity-id"
    }
  },
  "conditions": [
    {
      "kind": "when|unless",
      "body": "condition expression"
    }
  ]
}
```

### Schema Format

Schemas define the structure of entities and actions:

```json
{
  "": {
    "entityTypes": {
      "EntityType": {
        "shape": {
          "type": "Record|String|Boolean|Long|Set",
          "attributes": {
            "attr_name": {
              "type": "String|Boolean|Long|Record|Set",
              "required": true|false
            }
          }
        },
        "memberOfTypes": ["ParentType"]
      }
    },
    "actions": {
      "action_id": {
        "appliesTo": {
          "principalTypes": ["PrincipalType"],
          "resourceTypes": ["ResourceType"]
        }
      }
    }
  }
}
```

## Usage Examples

### Basic Workflow

1. **Set up the schema**:
```bash
curl -X PUT \
  -H "Content-Type: application/json" \
  -H "Authorization: your-api-key" \
  -d @schema.json \
  http://localhost:8180/v1/schema
```

2. **Add policies**:
```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: your-api-key" \
  -d @policy.json \
  http://localhost:8180/v1/policies
```

3. **Add data/entities**:
```bash
curl -X PUT \
  -H "Content-Type: application/json" \
  -H "Authorization: your-api-key" \
  -d @entities.json \
  http://localhost:8180/v1/data
```

4. **Check authorization**:
```bash
curl -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: your-api-key" \
  -d @auth_request.json \
  http://localhost:8180/v1/is_authorized
```

### Configuration

Configure the server using environment variables or command line arguments:

```bash
# Set port
export CEDAR_AGENT_PORT=8180

# Set authentication token
export CEDAR_AGENT_AUTHENTICATION=your-secret-key

# Load schema from file on startup
export CEDAR_AGENT_SCHEMA=schema.json

# Load data from file on startup
export CEDAR_AGENT_DATA=data.json

# Load policies from file on startup
export CEDAR_AGENT_POLICIES=policies.json

# Run the server
cargo run
```

## API Documentation UI

The Cedar Agent provides interactive API documentation:

- **Swagger UI**: http://localhost:8180/swagger-ui/
- **RapiDoc**: http://localhost:8180/rapidoc/

These interfaces provide detailed information about all endpoints, including request/response schemas, parameters, and example requests.

## Development

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Running

```bash
cargo run -- --port 8180 --authentication your-key
```

For more information, see the [README.md](README.md) file.
