package example.consumer

import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.Headers
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import kotlin.test.*
import kotlin.time.Instant
import kotlinx.serialization.json.*

class Protocol_json10_Test {
    @Test fun generatedJsonProtocol() = realTest {
        val rest=false
        val content="application/x-amz-json-1.0"
        var mode="echo"
        val http=HttpClient(MockEngine { request ->
            assertEquals("POST",request.method.value)
            assertEquals(if(rest) if(mode=="empty") "/empty" else "/echo" else "/",request.url.encodedPath)
            if(!rest) assertEquals("JsonService."+if(mode=="empty")"Empty" else "Echo",request.headers["x-amz-target"])
            val bodyText=(request.body as? OutgoingContent.ByteArrayContent)?.bytes()?.decodeToString() ?: ""
            if(mode=="echo") {
                assertTrue((request.headers["content-type"] ?: request.body.contentType.toString()).startsWith(content))
                val body=Json.parseToJsonElement(bodyText).jsonObject
                assertEquals("value",body[if(rest)"wire-name" else "renamed"]!!.jsonPrimitive.content)
                assertEquals(-0.25,body["when"]!!.jsonPrimitive.double)
                assertEquals("1969-12-31T23:59:59.750Z",body["iso"]!!.jsonPrimitive.content)
                assertFalse(body.containsKey("nullable"));assertTrue(body.containsKey("document"));assertEquals(JsonNull,body["document"])
                assertEquals("NaN",body["floatValue"]!!.jsonPrimitive.content);assertEquals("Infinity",body["doubleValue"]!!.jsonPrimitive.content)
                if(rest) {
                    assertFalse(body.containsKey("date"));assertFalse(body.containsKey("query"))
                    assertEquals("Thu, 01 Jan 1970 00:00:00 GMT",request.headers["x-date"])
                    assertEquals("Thu, 01 Jan 1970 00:00:00 GMT, Thu, 01 Jan 1970 00:00:01 GMT",request.headers["x-dates"])
                    assertEquals("1970-01-01T00:00:00Z",request.url.parameters["time"])
                } else { assertEquals(0,body["date"]!!.jsonPrimitive.int);assertEquals(2,body["dates"]!!.jsonArray.size) }
            } else if(mode=="empty" && !rest) assertEquals("{}",bodyText)
            var status=HttpStatusCode.OK
            var response="""{"choice":{"__type":"metadata","time":0.25},"floatValue":"NaN","doubleValue":"-Infinity","nullable":null,"document":null}"""
            val headers=Headers.build {
                if(rest) { append("x-date","Thu, 01 Jan 1970 00:00:00 GMT");append("x-dates","Thu, 01 Jan 1970 00:00:00 GMT, Thu, 01 Jan 1970 00:00:01 GMT") }
                if(mode=="errorHeader") append("x-amzn-errortype","Problem:https://example.test")
                append("x-amzn-requestid","request-123")
            }
            if(mode=="empty") { response="{}";if(rest)status=HttpStatusCode.NoContent }
            if(mode.startsWith("error")) {
                status=HttpStatusCode.Conflict
                response=when(mode) {
                    "errorType" -> """{"__type":"example.json#Problem","Message":"failure"}"""
                    "errorCode" -> """{"code":"Problem","Message":"failure"}"""
                    else -> """{"Message":"failure"}"""
                }
            }
            respond(response,status,headers)
        })
        val negative=Instant.fromEpochSeconds(-1,750000000);val zero=Instant.fromEpochSeconds(0)
        example.json.json10.JsonServiceClient(example.json.json10.ClientConfig(endpoint="https://example.test",httpClient=http,maxAttempts=1)).use { client ->
            val result=client.echo {
                renamed="value";`when`=negative;iso=negative;date=zero;dates=listOf(zero,Instant.fromEpochSeconds(1));query=zero;queries=listOf(zero)
                choice=example.json.json10.model.Choice.Time(zero);floatValue=Float.NaN;doubleValue=Double.POSITIVE_INFINITY;nullable=null;document=JsonNull
            }
            assertEquals(negative,result.`when`);assertEquals(negative,result.iso)
            assertEquals(Instant.fromEpochSeconds(0,250000000),(result.choice as example.json.json10.model.Choice.Time).value)
            assertTrue(result.floatValue!!.isNaN());assertEquals(Double.NEGATIVE_INFINITY,result.doubleValue);assertNull(result.nullable)
            if(rest) {assertEquals(zero,result.date);assertEquals(listOf(zero,Instant.fromEpochSeconds(1)),result.dates)}
            mode="empty";client.empty()
            for(error in listOf("errorHeader","errorType","errorCode")) {
                mode=error
                val failure=assertFailsWith<example.json.json10.model.Problem> {client.echo()}
                assertEquals("failure",failure.message);assertEquals("request-123",failure.requestId)
            }
        }
        http.close()
    }
}
